//! Replica sync: pull commits, cursors, outbox, sync functions.


// ---- apply ----
// Pull-page commit: one owner method, one transaction, everything inline.
//
// No helper here takes a transaction or a connection. The commit method
// below opens its transaction and carries every statement inline so the
// atomic unit (acknowledge outbox mutations plus apply the incoming page)
// is visible in exactly one place. What stays in separate functions is
// pure: wire decoding, version comparison inputs, and DTO mapping, none of
// which touches storage.

use crate::placements::PlacementsSql;
use crate::{Database, DatabaseError};
use include_sqlite_sql::include_sql;
use library_replica::{DirectoryIntent, DirectoryLifecycleState, MutationBody};
use rusqlite::OptionalExtension;
use sync_common::{MutationId, PullStateResponse};

include_sql!("src/sync/sync_apply.sql");

pub(crate) fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

pub(crate) fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

pub(crate) fn nonnegative_u64(value: i64, column: usize, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} must not be negative")))
}

pub(crate) fn bounded_u8(value: i64, column: usize, name: &str) -> rusqlite::Result<u8> {
    u8::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} is outside its storage range")))
}

pub(crate) fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

pub(crate) fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

#[derive(Clone, Debug)]
struct StoredVersion(library_replica::VersionKey);

impl StoredVersion {
    fn decode(changed_at: i64, conflict_rank: i64, actor: String, actor_seq: i64, mutation_id: String, columns: [usize; 5]) -> rusqlite::Result<Option<Self>> {
        if changed_at == 0 && actor.is_empty() && actor_seq == 0 && mutation_id.is_empty() {
            return Ok(None);
        }
        let mutation_id = MutationId::parse(&mutation_id).map_err(|error| invalid_text_column(columns[4], error.to_string()))?;
        let changed_at = nonnegative_u64(changed_at, columns[0], "change timestamp")?;
        let conflict_rank = bounded_u8(conflict_rank, columns[1], "conflict rank")?;
        let actor_seq = nonnegative_u64(actor_seq, columns[3], "actor sequence")?;
        let actor = parse_uuid_column(actor, columns[2])?;
        Ok(Some(Self(library_replica::VersionKey::new(changed_at, conflict_rank, actor, actor_seq, mutation_id))))
    }

    fn into_key(self) -> library_replica::VersionKey {
        self.0
    }

    fn decode_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<Option<library_replica::VersionKey>> {
        Self::decode(row.get(offset)?, row.get(offset + 1)?, row.get(offset + 2)?, row.get(offset + 3)?, row.get(offset + 4)?, [offset, offset + 1, offset + 2, offset + 3, offset + 4]).map(|version| version.map(Self::into_key))
    }
}

/// Decoded wire values and state identities only. Never snapshots database
/// state: winning versions and lifecycle checks still run under the writer.
struct PreparedRemoteChanges<'a> {
    changes: Vec<(&'a sync_common::ServerMutation, MutationBody, (&'static str, String, String))>,
}

fn prepare_remote_changes(changes: &[sync_common::ServerMutation]) -> Result<PreparedRemoteChanges<'_>, Box<dyn std::error::Error>> {
    let mut prepared = Vec::with_capacity(changes.len());
    for change in changes {
        if change.mutation.kind == sync_common::mutation_kind::CATALOG_BOOK {
            log::debug!("Skipping retired catalog-book mutation");
            continue;
        }
        if let Some(body) = MutationBody::from_wire(&change.mutation)? {
            let identity = sync_state_identity(&body);
            prepared.push((change, body, identity));
        } else {
            log::debug!("Skipping unknown synchronized state kind {}", change.mutation.kind);
        }
    }
    Ok(PreparedRemoteChanges { changes: prepared })
}

fn sync_state_identity(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are filtered before decoding"),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
        MutationBody::PdfReaderMetadata { content_hash, value } => ("pdf_reader_metadata", content_hash.to_string(), value.checksum.clone()),
        MutationBody::Description { content_hash, .. } => ("description", content_hash.to_string(), String::new()),
        MutationBody::BookToc { content_hash, .. } => ("book_toc", content_hash.to_string(), String::new()),
    }
}

/// Read snapshots only for independent additions; mixed pages retain ordered reads.
struct BookSnapshot {
    version: Option<library_replica::VersionKey>,
    has_metadata: bool,
    is_purged: bool,
    exists: bool,
}

fn independent_additions(changes: &PreparedRemoteChanges<'_>) -> bool {
    let mut seen = std::collections::HashSet::new();
    !changes.changes.is_empty() && changes.changes.iter().all(|(_, body, _)| matches!(body, MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { .. } } if seen.insert(*content_hash)))
}

fn independent_metadata(changes: &PreparedRemoteChanges<'_>) -> bool {
    let mut seen = std::collections::HashSet::new();
    !changes.changes.is_empty()
        && changes.changes.iter().all(|(_, body, _)| matches!(body, MutationBody::Metadata { content_hash, .. } if seen.insert(*content_hash)))
}

fn incoming_version(change: &sync_common::ServerMutation) -> library_replica::VersionKey {
    library_replica::VersionKey::new(
        change.mutation.changed_at,
        change.mutation.conflict_rank,
        change.replica_id.clone(),
        change.mutation.replica_seq.get(),
        change.mutation.mutation_id,
    )
}

/// Single-statement snapshot read, same shape as the cursor helpers: one
/// cached statement, no transaction orchestration.
fn read_book_snapshots(conn: &rusqlite::Connection, keys_json: &str) -> Result<Vec<BookSnapshot>, DatabaseError> {
    let mut snapshots = Vec::new();
    conn.book_addition_snapshots(keys_json, |row| {
        snapshots.push(BookSnapshot {
            has_metadata: row.get(0)?,
            version: if row.get::<_, Option<i64>>(1)?.is_some() { StoredVersion::decode_row(row, 1)? } else { None },
            is_purged: row.get(6)?,
            exists: row.get(7)?,
        });
        Ok(())
    })
    .map_err(DatabaseError::operation)?;
    Ok(snapshots)
}

pub(crate) fn toc_entry_count(entries: &[library_model::BookTocEntry]) -> usize {
    entries.iter().map(|entry| 1 + toc_entry_count(&entry.children)).sum()
}

fn parse_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, DatabaseError> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(|error| DatabaseError::operation(format!("stored revision is not readable: {error}")))?;
            sync_common::LibraryRevision::new(raw).map_err(|error| DatabaseError::operation(format!("stored revision is not readable: {error}")))
        })
        .transpose()
}

fn read_cursor_revision(conn: &rusqlite::Connection, key: PullCursorKey) -> Result<Option<String>, DatabaseError> {
    // Single-statement read used twice by the commit (pre-transaction fast
    // path and in-transaction recheck). It takes the connection rather than
    // the transaction: it never manages transaction boundaries.
    let mut revision = None;
    match key {
        PullCursorKey::State => conn.cursor_state_select(|row| {
            revision = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?,
        PullCursorKey::Reading => conn.cursor_reading_select(|row| {
            revision = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?,
    };
    Ok(revision.flatten())
}

#[derive(Clone, Copy)]
enum PullCursorKey {
    State,
    Reading,
}

fn write_cursor_revision(tx: &rusqlite::Connection, key: PullCursorKey, revision: Option<sync_common::LibraryRevision>) -> Result<(), DatabaseError> {
    // Counterpart to the read above: one statement, no boundary management.
    // It takes the connection rather than the transaction for the same
    // reason; the commit passes its transaction, which dereferences.
    match (key, revision) {
        (PullCursorKey::State, Some(revision)) => {
            tx.cursor_state_upsert(&revision.get().to_string()).map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::State, None) => {
            tx.cursor_state_clear().map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::Reading, Some(revision)) => {
            tx.cursor_reading_upsert(&revision.get().to_string()).map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::Reading, None) => {
            tx.cursor_reading_clear().map_err(DatabaseError::operation)?;
        }
    }
    Ok(())
}

// --- Metadata projection helpers. All pure: no statement here touches
// storage. The commit method below runs every write inline. ---

pub(crate) fn decode_book_metadata(raw: &[u8]) -> Result<book_model::BookMetadata, DatabaseError> {
    let invalid = |error: String| DatabaseError::operation(format!("stored book payload is not readable: {error}"));
    if raw.is_empty() {
        return Ok(book_model::BookMetadata::default());
    }
    match sync_common::wire::decode(raw, sync_common::wire::MAX_DECODED_REQUEST_BYTES) {
        Ok(metadata) => Ok(metadata),
        Err(sync_common::wire::WireError::UnsupportedVersion(version @ (10 | 11))) if raw.starts_with(b"BKHM") && raw.get(4) == Some(&version) => {
            let mut compatible = raw.to_vec();
            compatible[4] = sync_common::wire::header_bytes()[4];
            let metadata = sync_common::wire::decode::<book_model::BookMetadata>(&compatible, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid(error.to_string()))?;
            let reencoded = sync_common::wire::encode(&metadata).map_err(|error| invalid(error.to_string()))?;
            if reencoded.get(5..) != raw.get(5..) {
                return Err(invalid(format!("wire version {version} book payload is not byte-compatible with the current wire version")));
            }
            log::info!("upgrading a byte-compatible book metadata value from wire version {version}");
            Ok(metadata)
        }
        Err(error) => Err(invalid(error.to_string())),
    }
}

pub(crate) fn normalized_description(description: &str) -> String {
    let mut normalized = String::with_capacity(description.len().min(65_536));
    for character in description.chars().filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t')) {
        if normalized.len() + character.len_utf8() > 65_536 {
            break;
        }
        normalized.push(character);
    }
    normalized
}

pub(crate) fn identifier_scheme_label(scheme: &book_model::Scheme) -> String {
    use book_model::Scheme::*;
    match scheme {
        Unspecified => "unspecified".to_owned(),
        Isbn => "isbn".to_owned(),
        Asin => "asin".to_owned(),
        Doi => "doi".to_owned(),
        Issn => "issn".to_owned(),
        Oclc => "oclc".to_owned(),
        Lccn => "lccn".to_owned(),
        Uuid => "uuid".to_owned(),
        Calibre => "calibre".to_owned(),
        Google => "google".to_owned(),
        Goodreads => "goodreads".to_owned(),
        Kobo => "kobo".to_owned(),
        Other(value) => format!("other:{value}"),
    }
}

pub(crate) fn identifier_scope_label(scope: book_model::Scope) -> &'static str {
    use book_model::Scope::*;
    match scope {
        Book => "book",
        Edition => "edition",
        Work => "work",
    }
}

pub(crate) fn parse_author_name_opt(author_name: &str) -> Result<Option<catalog_model::AuthorName>, DatabaseError> {
    if author_name.trim().is_empty() {
        return Ok(None);
    }
    let name = catalog_model::AuthorName::parse(author_name).map_err(|error| DatabaseError::operation(format!("author name is not readable: {error}")))?;
    if name.match_key().as_str().is_empty() {
        return Ok(None);
    }
    Ok(Some(name))
}

pub(crate) fn unix_millis() -> Result<i64, DatabaseError> {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_millis();
    i64::try_from(millis).map_err(DatabaseError::operation)
}

/// CPU-only book projection, built before the writer runs its
/// statements. Mirrors the enrichment write path: identical inputs project
/// identical rows.
pub(crate) struct PreparedBook {
    pub(crate) metadata: book_model::BookMetadata,
    pub(crate) encoded: Vec<u8>,
    pub(crate) subject_codes: Vec<subject_projection::ProjectedSubjectCode>,
    pub(crate) assignments: Vec<subject_projection::ProjectedSubjectAssignment>,
    pub(crate) concepts: Vec<(usize, i64, String)>,
    pub(crate) references: Vec<(String, Option<String>, Option<String>)>,
    pub(crate) previous: Option<book_model::BookMetadata>,
    pub(crate) classification_rows: Vec<ClassificationRows>,
}

pub(crate) fn project_book(metadata: &book_model::BookMetadata, encoded: Vec<u8>, previous: Option<&book_model::BookMetadata>) -> Result<PreparedBook, DatabaseError> {
    let invalid = |error: String| DatabaseError::operation(format!("book projection failed: {error}"));
    let subjects_changed = previous.is_none_or(|old| old.subjects != metadata.subjects);
    let properties_changed = previous.is_none_or(|old| old.properties != metadata.properties);
    let mut metadata = metadata.clone();
    metadata.infer_embedded_subject_isbns().map_err(|e| invalid(e.to_string()))?;
    let subject_codes = if subjects_changed { subject_projection::project_subject_codes(&metadata.subjects) } else { Vec::new() };
    let assignments = if subjects_changed { subject_projection::project_subjects(&metadata.subjects) } else { Vec::new() };
    let mut concepts = Vec::new();
    for (index, code) in subject_codes.iter().enumerate() {
        for path in subject_projection::unified_subject_paths(code.system_id(), code.code()) {
            concepts.push((index, subject_projection::unified_concept_id(&path).expect("projected subject exists in the taxonomy"), path));
        }
    }
    let references = if properties_changed {
        book_enrichment::related_isbn::lookup_candidates(&metadata.properties).into_iter().map(|isbn| (isbn, None, None)).collect()
    } else {
        Vec::new()
    };
    let previous = previous
        .cloned()
        .map(|mut old| {
            old.infer_embedded_subject_isbns().map_err(|e| invalid(e.to_string()))?;
            Ok::<_, DatabaseError>(old)
        })
        .transpose()?;
    let mut prepared = PreparedBook { metadata, encoded, subject_codes, assignments, concepts, references, previous, classification_rows: Vec::new() };
    if subjects_changed {
        prepared.classification_rows = prepare_classification_rows(&prepared);
    }
    Ok(prepared)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cell {
    Null,
    Integer(i64),
    Text(String),
}

impl From<i64> for Cell {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<&str> for Cell {
    fn from(v: &str) -> Self {
        Self::Text(v.into())
    }
}

impl From<Option<&str>> for Cell {
    fn from(v: Option<&str>) -> Self {
        v.map_or(Self::Null, Self::from)
    }
}

impl rusqlite::types::ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        use rusqlite::types::{ToSqlOutput, Value, ValueRef};
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(Value::Null),
            Self::Integer(v) => ToSqlOutput::Owned(Value::Integer(*v)),
            Self::Text(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
        })
    }
}

pub(crate) struct ClassificationRows {
    pub(crate) table: &'static str,
    pub(crate) keys: usize,
    pub(crate) width: usize,
    pub(crate) desired: std::collections::BTreeMap<Vec<Cell>, Vec<Cell>>,
}

pub(crate) fn prepare_classification_rows(prepared: &PreparedBook) -> Vec<ClassificationRows> {
    let subjects = prepared
        .metadata
        .subjects
        .iter()
        .enumerate()
        .map(|(i, s)| vec![(i as i64).into(), s.name().into(), s.source().into(), s.authority().into(), s.code().into()])
        .collect();
    let mut codes = Vec::new();
    for code in &prepared.subject_codes {
        for pos in code.evidence_positions() {
            codes.push(vec![(*pos as i64).into(), code.system_id().into(), code.code().into()]);
        }
    }
    let mut assignments = Vec::new();
    let mut evidence = Vec::new();
    for assignment in &prepared.assignments {
        assignments.push(vec![assignment.system_id().into(), assignment.subject_path().into(), assignment.matcher_version().into()]);
        for pos in assignment.evidence_positions() {
            evidence.push(vec![assignment.system_id().into(), assignment.subject_path().into(), (*pos as i64).into()]);
        }
    }
    let mut concepts = Vec::new();
    let mut unified_evidence = Vec::new();
    for (index, concept, _) in &prepared.concepts {
        concepts.push(vec![(*concept).into(), subject_projection::UNIFIED_MATCHER_VERSION.into()]);
        let code = &prepared.subject_codes[*index];
        for pos in code.evidence_positions() {
            unified_evidence.push(vec![(*concept).into(), (*pos as i64).into(), code.system_id().into(), code.code().into()]);
        }
    }
    fn rows(table: &'static str, keys: usize, width: usize, rows: Vec<Vec<Cell>>) -> ClassificationRows {
        ClassificationRows { table, keys, width, desired: rows.into_iter().map(|v| (v[..keys].to_vec(), v)).collect() }
    }
    vec![
        rows("book_subject", 1, 5, subjects),
        rows("book_subject_code", 3, 3, codes),
        rows("book_subject_assignment", 2, 3, assignments),
        rows("book_subject_assignment_evidence", 3, 3, evidence),
        rows("book_unified_concept", 1, 2, concepts),
        rows("book_unified_concept_evidence", 4, 4, unified_evidence),
    ]
}

pub(crate) fn source_subject_version(system_id: &str) -> &'static str {
    match system_id {
        subject_projection::BISAC_SYSTEM_ID => subject_projection::BISAC_VERSION,
        subject_projection::LCC_SYSTEM_ID => subject_projection::LCC_VERSION,
        _ => "external",
    }
}

pub(crate) fn source_mapping_type(system_id: &str) -> &'static str {
    match system_id {
        subject_projection::BISAC_SYSTEM_ID => "exact",
        subject_projection::LCC_SYSTEM_ID => "broader",
        _ => "related",
    }
}

#[cfg(test)]
mod apply_tests {
    use crate::*;
    use library_replica::{MutationBody, ReadingPositionState, StateMutation, SyncBookMetadata};
    use sync_common::{LibraryRevision, ReplicaSeq, SyncCursor};

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-apply-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    fn mutation_id(tag: &str) -> MutationId {
        MutationId::parse(&format!("12345678-1234-1234-1234-{tag:0>12}")).unwrap()
    }

    fn placement_change(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::ServerMutation {
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-333333333333").unwrap();
        let body = MutationBody::Placement { dir_id, content_hash: hash, present: true, origin_folder_id: None };
        let mutation = StateMutation { mutation_id: mutation_id("333333333333"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    fn reading_change(hash: sync_common::ContentHash, cfi: &str, progress: f32, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::ReadingPosition {
            content_hash: hash,
            value: ReadingPositionState { location: book_model::ReadingPosition::parse(cfi).unwrap(), progress },
        };
        let mutation = StateMutation { mutation_id: mutation_id("111111111111"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    fn metadata_change(hash: sync_common::ContentHash, title: &str, author: &str, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::Metadata {
            content_hash: hash,
            value: SyncBookMetadata {
                title: title.to_owned(),
                contributors: vec![book_model::Contributor::new(author, None, book_model::MarcRelatorCode(*b"aut")).unwrap()],
                book: book_model::BookMetadata::default(),
            },
        };
        let mutation = StateMutation { mutation_id: mutation_id("222222222222"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    #[test]
    fn pull_commit_applies_reading_advances_cursor_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"7".repeat(64));
        let next = SyncCursor { state_revision: None, reading_revision: Some(LibraryRevision::new(3).unwrap()) };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), reading_change(hash, "epubcfi(/6/4)", 0.5, 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit reading page"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), next);
        assert_eq!(database.browse_book_detail(hash).expect("read detail").book.progress, 0.5);
        let stale = PullStateResponse { mutations: vec![reading_change(hash, "epubcfi(/6/2)", 0.9, 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &stale).expect("commit stale page"));
        assert_eq!(database.browse_book_detail(hash).expect("reread detail").book.progress, 0.5);
    }

    #[test]
    fn pull_commit_projects_metadata_identity_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"8".repeat(64));
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(5).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), metadata_change(hash, "Remote Title", "Remote Author", 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit metadata page"));
        let detail = database.browse_book_detail(hash).expect("read detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "incoming contributor is projected");
        let stale = PullStateResponse { mutations: vec![metadata_change(hash, "Stale Title", "Stale Author", 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &stale).expect("commit stale page"));
        let detail = database.browse_book_detail(hash).expect("reread detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "stale contributor never lands");
    }

    fn batch_hash(index: u64) -> sync_common::ContentHash {
        sync_common::ContentHash::new(&format!("{index:064x}"))
    }

    fn batch_metadata(index: u64) -> SyncBookMetadata {
        SyncBookMetadata {
            title: format!("Batch Title {index}"),
            contributors: vec![book_model::Contributor::new(format!("Batch Author {index}"), None, book_model::MarcRelatorCode(*b"aut")).unwrap()],
            book: book_model::BookMetadata::default(),
        }
    }

    fn batch_mutation(tag: String, body: MutationBody, changed_at: u64, replica: u128) -> sync_common::ServerMutation {
        let mutation = StateMutation { mutation_id: mutation_id(&tag), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::from_u128(replica), revision: LibraryRevision::new(1).unwrap() }
    }

    fn addition_change(index: u64, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::BookLifecycle {
            content_hash: batch_hash(index),
            value: library_replica::BookLifecycleState::Present { metadata: batch_metadata(index), added_at: changed_at, format: book_model::BookFormat::Epub },
        };
        batch_mutation(format!("{index:012}"), body, changed_at, u128::from(index))
    }

    fn removal_change(hash: sync_common::ContentHash, tag: &str, changed_at: u64, purged: bool) -> sync_common::ServerMutation {
        let value = if purged {
            library_replica::BookLifecycleState::Purged
        } else {
            library_replica::BookLifecycleState::Deleted { origin_folder_id: None }
        };
        batch_mutation(tag.to_owned(), MutationBody::BookLifecycle { content_hash: hash, value }, changed_at, 9_999_999)
    }

    fn state_snapshot(database: &Database) -> (Vec<(String, Option<String>, bool, bool)>, Vec<(String, String, String, i64, i64, i64)>, Vec<String>, Vec<(String, String, i64)>, SyncCursor) {
        let books = database
            .connection
            .prepare("SELECT content_hash, title, deleted_at IS NOT NULL, purged_at IS NOT NULL FROM book ORDER BY content_hash")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let versions = database
            .connection
            .prepare("SELECT state_kind, state_key, state_subkey, changed_at, conflict_rank, replica_seq FROM sync_state_version ORDER BY state_kind, state_key, state_subkey")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let work = database
            .connection
            .prepare("SELECT content_hash FROM local_book_work ORDER BY content_hash")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        let contributors = database
            .connection
            .prepare("SELECT b.content_hash, c.name, c.position FROM book_contributor c JOIN book b ON b.row_id = c.book_row_id ORDER BY b.content_hash, c.position")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (books, versions, work, contributors, database.sync_pull_cursor().expect("read cursor"))
    }

    fn decoy_hash() -> sync_common::ContentHash {
        sync_common::ContentHash::new(&"f".repeat(64))
    }

    #[test]
    fn batched_additions_match_ordered_across_bounded_snapshots() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Seeds land identically through the ordered path on both sides.
        // Index 200 is newer than the page, 201 older, 202 deleted, 203
        // purged, 204 owned by a newer metadata change.
        let seed = vec![
            addition_change(200, 110),
            addition_change(201, 10),
            addition_change(202, 10),
            removal_change(batch_hash(202), "000000000900", 20, false),
            addition_change(203, 10),
            removal_change(batch_hash(203), "000000000901", 20, true),
            addition_change(204, 10),
            metadata_change(batch_hash(204), "Metadata owns this title", "Metadata Author", 120),
            placement_change(decoy_hash(), 90),
        ];
        let next_seed = SyncCursor { state_revision: Some(LibraryRevision::new(2).unwrap()), reading_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.sync_commit_pull_response(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::ServerMutation> = (0..205).map(|index| addition_change(index, 100)).collect();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.sync_commit_pull_response(&[], &page).expect("commit batched page"));
        // One stale change forces the whole page onto the ordered path but
        // loses its version check, so it must not perturb the outcome.
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.sync_commit_pull_response(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, _, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _, _)| title.clone());
        assert_eq!(title(200), Some(Some("Batch Title 200".to_owned())), "seed title survives, page title is stale");
        assert_eq!(title(201), Some(Some("Batch Title 201".to_owned())));
        assert_eq!(title(204), Some(Some("Metadata owns this title".to_owned())), "metadata-owned title survives the top-up");
        assert_eq!(title(203), Some(Some("Batch Title 203".to_owned())), "newer lifecycle wins revive the purged tombstone");
        assert!(books.iter().all(|(hash, _, _, purged)| *hash != batch_hash(203).as_str() || !purged), "revived book is not purged");
    }

    #[test]
    fn batched_metadata_matches_ordered_for_missing_and_existing_books() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Even indexes exist up front; odd indexes arrive through ensure.
        // Index 100 is newer than the page, 101 purged, 102 carries a title
        // that a metadata change must still overwrite.
        let mut seed = Vec::new();
        for index in (0..100).step_by(2) {
            seed.push(placement_change(batch_hash(index), 90));
        }
        seed.push(metadata_change(batch_hash(100), "Newer owns this title", "Newer Author", 110));
        seed.push(addition_change(101, 10));
        seed.push(removal_change(batch_hash(101), "000000000902", 20, true));
        seed.push(addition_change(102, 10));
        seed.push(placement_change(decoy_hash(), 90));
        let next_seed = SyncCursor { state_revision: Some(LibraryRevision::new(2).unwrap()), reading_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.sync_commit_pull_response(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::ServerMutation> = (0..103).map(|index| metadata_change(batch_hash(index), &format!("Batch Title {index}"), &format!("Meta Author {index}"), 100)).collect();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.sync_commit_pull_response(&[], &page).expect("commit batched page"));
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.sync_commit_pull_response(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, contributors, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _, _)| title.clone());
        assert_eq!(title(102), Some(Some("Batch Title 102".to_owned())));
        assert!(contributors.iter().any(|(hash, name, _)| *hash == batch_hash(102).as_str() && *name == "Meta Author 102"), "metadata overwrites despite stored title");
        assert_eq!(title(1), Some(Some("Batch Title 1".to_owned())), "missing book arrives through ensure");
        assert_eq!(title(100), Some(Some("Newer owns this title".to_owned())));
        assert!(books.iter().any(|(hash, title, _, purged)| *hash == batch_hash(101).as_str() && *purged && title.is_none()), "purged book stays purged");
    }

    fn directory_change(body: MutationBody, changed_at: u64, tag: &str) -> sync_common::ServerMutation {
        let mutation = StateMutation { mutation_id: mutation_id(tag), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    #[test]
    fn pull_commit_applies_directory_intents_and_projection() {
        let holder = open_test_database();
        let database = &holder.database;
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-444444444444").unwrap();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(6).unwrap()), reading_revision: None };
        let page = PullStateResponse {
            mutations: vec![
                directory_change(MutationBody::DirectoryName { dir_id, name: "Shelf".to_owned() }, 10, "444444444441"),
                directory_change(MutationBody::DirectoryParent { dir_id, parent_id: sync_common::ROOT_DIR_ID }, 20, "444444444442"),
                directory_change(MutationBody::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Present }, 30, "444444444443"),
            ],
            next_cursor: next.clone(),
            has_more: false,
        };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit directory page"));
        let row: (String, String, i64, String, String) = database
            .connection
            .prepare("SELECT intent_name, intent_parent_id, intent_lifecycle, name, parent_id FROM dir WHERE id = ?1")
            .unwrap()
            .query_row([dir_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .unwrap();
        assert_eq!(row.0, "Shelf");
        assert_eq!(row.1, sync_common::ROOT_DIR_ID.to_string());
        assert_eq!(row.2, 0);
        assert_eq!(row.3, "Shelf", "projected name follows the intent");
        assert_eq!(row.4, sync_common::ROOT_DIR_ID.to_string());
    }

    #[test]
    fn pull_commit_stores_description_and_marks_scanned() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"9".repeat(64));
        let body = MutationBody::Description { content_hash: hash, value: "A\x00noisy\x07 description.".to_owned() };
        let mutation = StateMutation { mutation_id: mutation_id("555555555555"), body, changed_at: 100, replica_seq: ReplicaSeq::new(1).unwrap() };
        let change = sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() };
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(7).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![change], next_cursor: next, has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit description page"));
        let stored: (String, i64) = database
            .connection
            .prepare("SELECT d.description, b.description_scanned FROM book_description d JOIN book b ON b.row_id = d.book_row_id WHERE b.content_hash = ?1")
            .unwrap()
            .query_row([hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        assert_eq!(stored.0, "Anoisy description.");
        assert_eq!(stored.1, 1);
    }

    #[test]
    fn pull_commit_projects_subject_classification() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let mut book = book_model::BookMetadata::default();
        book.subjects = vec![book_model::BookSubject::new(None, "Distributed systems", "dc:subject", None, None).unwrap()];
        let body = MutationBody::Metadata {
            content_hash: hash,
            value: SyncBookMetadata {
                title: "Classified".to_owned(),
                contributors: vec![book_model::Contributor::new("Subject Author", None, book_model::MarcRelatorCode(*b"aut")).unwrap()],
                book,
            },
        };
        let mutation = StateMutation { mutation_id: mutation_id("666666666666"), body, changed_at: 100, replica_seq: ReplicaSeq::new(1).unwrap() };
        let change = sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() };
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(8).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), change], next_cursor: next, has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit classified page"));
        let subjects: Vec<String> = database
            .connection
            .prepare("SELECT s.name FROM book_subject s JOIN book b ON b.row_id = s.book_row_id WHERE b.content_hash = ?1 ORDER BY s.position")
            .unwrap()
            .query_map([hash.as_str()], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(subjects.iter().any(|subject| subject.contains("Distributed systems")), "subject survives projection, got {subjects:?}");
    }
}

impl Database {
    /// Acknowledges sent outbox mutations and applies one pull page under a
    /// single writer transaction. CPU-only wire decoding runs before the
    /// writer is reserved; a failed page rolls back the acknowledgements
    /// with it, so the caller re-acknowledges under a fresh transaction.
    /// Returns whether the page carried remote changes.
    pub fn sync_commit_pull_response(&self, ids: &[MutationId], page: &PullStateResponse) -> Result<bool, DatabaseError> {
        // Fast path without touching the writer: an empty page that changes
        // no cursor writes nothing.
        if page.mutations.is_empty() {
            let cursor = sync_common::SyncCursor {
                state_revision: parse_revision(read_cursor_revision(&self.connection, PullCursorKey::State)?)?,
                reading_revision: parse_revision(read_cursor_revision(&self.connection, PullCursorKey::Reading)?)?,
            };
            if cursor == page.next_cursor {
                return Ok(false);
            }
        }
        let prepared = prepare_remote_changes(&page.mutations).map_err(DatabaseError::operation)?;
        let tx = self.connection.unchecked_transaction().map_err(DatabaseError::operation)?;
        // Reserve the write lock up front, matching the previous writer.
        tx.reserve_write_lock().map_err(DatabaseError::operation)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON").map_err(DatabaseError::operation)?;
        if !ids.is_empty() {
            let payload = serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
            tx.outbox_delete_for_ids(&payload).map_err(DatabaseError::operation)?;
        }
        if page.mutations.is_empty() {
            let stored = sync_common::SyncCursor {
                state_revision: parse_revision(read_cursor_revision(&tx, PullCursorKey::State)?)?,
                reading_revision: parse_revision(read_cursor_revision(&tx, PullCursorKey::Reading)?)?,
            };
            if stored == page.next_cursor {
                tx.commit().map_err(DatabaseError::operation)?;
                return Ok(false);
            }
        }
        let mut previous_origin = None;
        tx.change_origin_select(|row| {
            previous_origin = row.get::<_, Option<String>>(0)?;
            Ok(())
        })
        .map_err(DatabaseError::operation)?;
        let previous_origin: Option<String> = previous_origin.flatten();
        let mut origin_remote = false;
        let mut reproject = false;
        let batch_additions = independent_additions(&prepared);
        let batch_metadata = !batch_additions && independent_metadata(&prepared);
        // One projection queue for every path. Ordered pages leave the
        // snapshot pair empty so the tail reads current rows; batch pages
        // pre-read one page at a time while lifecycle rows are fresh.
        let mut pending: Vec<(
            &sync_common::ContentHash,
            &library_replica::SyncBookMetadata,
            Option<(Option<book_model::BookMetadata>, bool)>,
        )> = Vec::new();
        if batch_additions {
            // All keys are distinct, so lifecycle rows can precede metadata and versions.
            let mut snapshots: Vec<BookSnapshot> = Vec::new();
            let mut books: Vec<(&str, &str, i64, &str)> = Vec::new();
            let mut versions: Vec<(&str, &str, &str, i64, i64, String, i64, String)> = Vec::new();
            let mut accepted: Vec<(&sync_common::ContentHash, &library_replica::SyncBookMetadata, bool)> = Vec::new();
            let mut queue: Vec<&str> = Vec::new();
            for (index, (change, body, identity)) in prepared.changes.iter().enumerate() {
                if index % 100 == 0 {
                    books.clear();
                    versions.clear();
                    accepted.clear();
                    queue.clear();
                    let page_end = (index + 100).min(prepared.changes.len());
                    let keys = serde_json::to_string(&prepared.changes[index..page_end].iter().map(|(_, _, identity)| identity).collect::<Vec<_>>())
                        .map_err(DatabaseError::operation)?;
                    snapshots = read_book_snapshots(&tx, &keys)?;
                }
                let snapshot = &snapshots[index % 100];
                if snapshot.version.as_ref().is_some_and(|stored| incoming_version(change) <= *stored) {
                    continue;
                }
                let MutationBody::BookLifecycle { content_hash, value: library_replica::BookLifecycleState::Present { metadata, added_at, format } } = body else {
                    unreachable!("independent additions checked by caller")
                };
                if !origin_remote {
                    tx.mark_origin_remote().map_err(DatabaseError::operation)?;
                    origin_remote = true;
                }
                books.push((
                    content_hash.as_str(),
                    metadata.title.as_str(),
                    storage_i64(*added_at, "book addition timestamp").map_err(DatabaseError::operation)?,
                    format.canonical_extension(),
                ));
                versions.push((
                    identity.0,
                    identity.1.as_str(),
                    identity.2.as_str(),
                    storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                    i64::from(change.mutation.conflict_rank),
                    change.replica_id.to_string(),
                    storage_i64(change.mutation.replica_seq.get(), "actor sequence").map_err(DatabaseError::operation)?,
                    change.mutation.mutation_id.to_string(),
                ));
                accepted.push((content_hash, metadata, snapshot.has_metadata));
                queue.push(content_hash.as_str());
                if (index + 1) % 100 != 0 && index + 1 != prepared.changes.len() {
                    continue;
                }
                if accepted.is_empty() {
                    continue;
                }
                tx.apply_remote_books_added_batch(&serde_json::to_string(&books).map_err(DatabaseError::operation)?)
                    .map_err(DatabaseError::operation)?;
                let projecting = accepted.iter().filter(|(_, _, has_metadata)| !has_metadata).map(|(hash, metadata, _)| (*hash, *metadata)).collect::<Vec<_>>();
                if !projecting.is_empty() {
                    let keys = serde_json::to_string(&projecting.iter().map(|(hash, _)| hash.as_str()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
                    let mut previous = Vec::new();
                    tx.book_batch_select(&keys, |row| {
                        let raw: Option<Vec<u8>> = row.get(0)?;
                        previous.push(raw.and_then(|raw| decode_book_metadata(&raw).ok()));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let mut matched = vec![true; projecting.len()];
                    let mut counts = vec![0usize; projecting.len()];
                    let mut contributor_rows = Vec::new();
                    tx.contributors_match_batch(&keys, |row| {
                        contributor_rows.push((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Vec<u8>>(5)?,
                        ));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    for (row_index, position_col, stable_id, name, sort_name, role) in contributor_rows {
                        let row_index = row_index as usize;
                        let position = counts[row_index];
                        counts[row_index] += 1;
                        let Some(credit) = projecting[row_index].1.contributors.get(position) else {
                            matched[row_index] = false;
                            continue;
                        };
                        matched[row_index] &= parse_author_name_opt(credit.name())?.is_some()
                            && position_col == position as i64
                            && stable_id.as_deref() == Some(credit.agent_id().to_string().as_str())
                            && name == credit.name()
                            && sort_name.as_deref() == credit.sort_name()
                            && role.as_slice() == credit.role_code().0;
                    }
                    for (entry_index, (_, metadata)) in projecting.iter().enumerate() {
                        matched[entry_index] &= counts[entry_index] == metadata.contributors.len();
                    }
                    for ((hash, metadata), (previous, matches)) in projecting.into_iter().zip(previous.into_iter().zip(matched)) {
                        pending.push((hash, metadata, Some((previous, matches))));
                    }
                }
                tx.record_versions_batch(&serde_json::to_string(&versions).map_err(DatabaseError::operation)?)
                    .map_err(DatabaseError::operation)?;
                tx.queue_book_work_batch(&serde_json::to_string(&queue).map_err(DatabaseError::operation)?)
                    .map_err(DatabaseError::operation)?;
            }
        } else if batch_metadata {
            let mut snapshots: Vec<BookSnapshot> = Vec::new();
            let mut missing: Vec<(&str, i64)> = Vec::new();
            let mut metadata: Vec<(&sync_common::ContentHash, &library_replica::SyncBookMetadata)> = Vec::new();
            let mut versions: Vec<(&str, &str, &str, i64, i64, String, i64, String)> = Vec::new();
            for (index, (change, body, identity)) in prepared.changes.iter().enumerate() {
                if index % 100 == 0 {
                    missing.clear();
                    metadata.clear();
                    versions.clear();
                    let page_end = (index + 100).min(prepared.changes.len());
                    let keys = serde_json::to_string(&prepared.changes[index..page_end].iter().map(|(_, _, identity)| identity).collect::<Vec<_>>())
                        .map_err(DatabaseError::operation)?;
                    snapshots = read_book_snapshots(&tx, &keys)?;
                }
                let snapshot = &snapshots[index % 100];
                if snapshot.version.as_ref().is_some_and(|stored| incoming_version(change) <= *stored) {
                    continue;
                }
                let MutationBody::Metadata { content_hash, value } = body else {
                    unreachable!("independent metadata checked by caller")
                };
                if !origin_remote {
                    tx.mark_origin_remote().map_err(DatabaseError::operation)?;
                    origin_remote = true;
                }
                versions.push((
                    identity.0,
                    identity.1.as_str(),
                    identity.2.as_str(),
                    storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                    i64::from(change.mutation.conflict_rank),
                    change.replica_id.to_string(),
                    storage_i64(change.mutation.replica_seq.get(), "actor sequence").map_err(DatabaseError::operation)?,
                    change.mutation.mutation_id.to_string(),
                ));
                if !snapshot.is_purged {
                    if !snapshot.exists {
                        missing.push((content_hash.as_str(), storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?));
                    }
                    metadata.push((content_hash, value));
                }
                if (index + 1) % 100 != 0 && index + 1 != prepared.changes.len() {
                    continue;
                }
                if versions.is_empty() {
                    continue;
                }
                tx.ensure_remote_books_batch(&serde_json::to_string(&missing).map_err(DatabaseError::operation)?)
                    .map_err(DatabaseError::operation)?;
                if !metadata.is_empty() {
                    let keys = serde_json::to_string(&metadata.iter().map(|(hash, _)| hash.as_str()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
                    let mut previous = Vec::new();
                    tx.book_batch_select(&keys, |row| {
                        let raw: Option<Vec<u8>> = row.get(0)?;
                        previous.push(raw.and_then(|raw| decode_book_metadata(&raw).ok()));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let mut matched = vec![true; metadata.len()];
                    let mut counts = vec![0usize; metadata.len()];
                    let mut contributor_rows = Vec::new();
                    tx.contributors_match_batch(&keys, |row| {
                        contributor_rows.push((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Vec<u8>>(5)?,
                        ));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    for (row_index, position_col, stable_id, name, sort_name, role) in contributor_rows {
                        let row_index = row_index as usize;
                        let position = counts[row_index];
                        counts[row_index] += 1;
                        let Some(credit) = metadata[row_index].1.contributors.get(position) else {
                            matched[row_index] = false;
                            continue;
                        };
                        matched[row_index] &= parse_author_name_opt(credit.name())?.is_some()
                            && position_col == position as i64
                            && stable_id.as_deref() == Some(credit.agent_id().to_string().as_str())
                            && name == credit.name()
                            && sort_name.as_deref() == credit.sort_name()
                            && role.as_slice() == credit.role_code().0;
                    }
                    for (entry_index, (_, incoming)) in metadata.iter().enumerate() {
                        matched[entry_index] &= counts[entry_index] == incoming.contributors.len();
                    }
                    for ((hash, incoming), (previous, matches)) in metadata.drain(..).zip(previous.into_iter().zip(matched)) {
                        pending.push((hash, incoming, Some((previous, matches))));
                    }
                }
                tx.record_versions_batch(&serde_json::to_string(&versions).map_err(DatabaseError::operation)?)
                    .map_err(DatabaseError::operation)?;
            }
        }
        for (change, body, (state_kind, state_key, state_subkey)) in &prepared.changes {
            if batch_additions || batch_metadata {
                // Handled by the batched pass above; this loop only runs for mixed pages.
                continue;
            }
            let incoming = library_replica::VersionKey::new(
                change.mutation.changed_at,
                change.mutation.conflict_rank,
                change.replica_id.clone(),
                change.mutation.replica_seq.get(),
                change.mutation.mutation_id,
            );
            let mut stored: Option<library_replica::VersionKey> = None;
            tx.read_sync_state_version_select(state_kind, state_key, state_subkey, |row| {
                stored = StoredVersion::decode_row(row, 0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            if stored.is_some_and(|stored| incoming <= stored) {
                continue;
            }
            if !origin_remote {
                tx.mark_origin_remote().map_err(DatabaseError::operation)?;
                origin_remote = true;
            }
            // Book metadata (lifecycle-present top-ups and metadata
            // mutations) shares one projection block after the match so the
            // identity/projection statements exist exactly once.
            let mut pending_metadata: Option<(&sync_common::ContentHash, &library_replica::SyncBookMetadata)> = None;
            match body {
                MutationBody::DirectoryName { dir_id, name } => {
                    if *dir_id != sync_common::ROOT_DIR_ID {
                        tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
                    }
                    tx.set_remote_directory_name(&dir_id.to_string(), name.as_str())
                        .map_err(DatabaseError::operation)?;
                    reproject = true;
                }
                MutationBody::DirectoryParent { dir_id, parent_id } => {
                    if *parent_id != sync_common::ROOT_DIR_ID {
                        tx.ensure_remote_directory_placeholder(&parent_id.to_string()).map_err(DatabaseError::operation)?;
                    }
                    if *dir_id != sync_common::ROOT_DIR_ID {
                        tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
                    }
                    tx.set_remote_directory_parent(&dir_id.to_string(), &parent_id.to_string())
                        .map_err(DatabaseError::operation)?;
                    reproject = true;
                }
                MutationBody::DirectoryLifecycle { dir_id, value } => {
                    if *dir_id != sync_common::ROOT_DIR_ID {
                        tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
                    }
                    let value = match value {
                        DirectoryLifecycleState::Present => 0,
                        DirectoryLifecycleState::Deleted => 1,
                        DirectoryLifecycleState::Purged => 2,
                    };
                    tx.set_remote_directory_lifecycle(&dir_id.to_string(), value)
                        .map_err(DatabaseError::operation)?;
                    reproject = true;
                }
                MutationBody::BookLifecycle { content_hash, value } => match value {
                    library_replica::BookLifecycleState::Present { metadata, added_at, format } => {
                        let mut has_metadata = false;
                        tx.remote_book_has_metadata(content_hash.as_str(), |row| {
                            has_metadata = row.get(0)?;
                            Ok(())
                        })
                        .map_err(DatabaseError::operation)?;
                        tx.apply_remote_book_added(
                            content_hash.as_str(),
                            metadata.title.as_str(),
                            storage_i64(*added_at, "book addition timestamp").map_err(DatabaseError::operation)?,
                            format.canonical_extension(),
                        )
                        .map_err(DatabaseError::operation)?;
                        if !has_metadata {
                            pending_metadata = Some((content_hash, metadata));
                        }
                    }
                    library_replica::BookLifecycleState::Deleted { origin_folder_id } => {
                        tx.apply_remote_book_deleted(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "book deletion timestamp").map_err(DatabaseError::operation)?,
                            origin_folder_id.as_deref(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                    library_replica::BookLifecycleState::Purged => {
                        tx.apply_remote_book_purged(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "book purge timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        let mut row_id: Option<i64> = None;
                        tx.apply_remote_book_purged_select(content_hash.as_str(), |row| {
                            row_id = row.get(0)?;
                            Ok(())
                        })
                        .map_err(DatabaseError::operation)?;
                        let row_id = row_id.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
                        tx.purge_book_publisher(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_identifier(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_language(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_contributor(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_subject_assignment_evidence(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_subject_assignment(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_subject(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_external_entity(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_external_metadata_attempt(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_collection(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_date(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_book_value(row_id).map_err(DatabaseError::operation)?;
                        tx.purge_annotation(row_id).map_err(DatabaseError::operation)?;
                        tx.apply_remote_book_purged_delete(row_id).map_err(DatabaseError::operation)?;
                        tx.apply_remote_book_purged_delete_2(content_hash.as_str()).map_err(DatabaseError::operation)?;
                        tx.apply_remote_book_purged_update(row_id).map_err(DatabaseError::operation)?;
                        tx.apply_remote_book_purged_delete_3(row_id).map_err(DatabaseError::operation)?;
                        tx.apply_remote_change_inner_insert(content_hash.as_str()).map_err(DatabaseError::operation)?;
                    }
                },
                MutationBody::Placement { dir_id, content_hash, present, origin_folder_id } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        if *dir_id != sync_common::ROOT_DIR_ID {
                            tx.ensure_remote_directory_placeholder(&dir_id.to_string()).map_err(DatabaseError::operation)?;
                        }
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "placement timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        if *present {
                            tx.apply_remote_book_dir_added(&dir_id.to_string(), content_hash.as_str())
                                .map_err(DatabaseError::operation)?;
                        } else {
                            tx.apply_remote_book_dir_removed(&dir_id.to_string(), content_hash.as_str(), origin_folder_id.as_deref())
                                .map_err(DatabaseError::operation)?;
                        }
                    }
                }
                MutationBody::ReadingPosition { content_hash, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        let progress = book_model::ReadProgress::new(value.progress).map_err(|error| DatabaseError::operation(error.to_string()))?;
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        tx.read_position_update(value.location.as_str(), progress.value(), content_hash.as_str())
                            .map_err(DatabaseError::operation)?;
                    }
                }
                MutationBody::Annotation { annotation_id, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(value.content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                    tx.first_book_insert(
                        value.content_hash.as_str(),
                        storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                    )
                    .map_err(DatabaseError::operation)?;
                    let style = match value.style {
                        book_model::AnnotationStyle::Highlight => "highlight",
                        book_model::AnnotationStyle::Underline => "underline",
                        book_model::AnnotationStyle::Squiggly => "squiggly",
                        book_model::AnnotationStyle::Strikethrough => "strikethrough",
                    };
                    let app_anchor = book_model::AnnotationAnchor::from(value.anchor.clone());
                    let anchor = crate::browse::stored_anchor(&app_anchor)?;
                    let created_at = storage_i64(value.created_at, "annotation creation timestamp").map_err(DatabaseError::operation)?;
                    let modified_at = storage_i64(value.modified_at, "annotation modification timestamp").map_err(DatabaseError::operation)?;
                    tx.upsert_remote_annotation(
                        annotation_id.as_str(),
                        value.content_hash.as_str(),
                        anchor.kind,
                        anchor.cfi.as_deref(),
                        anchor.page,
                        anchor.rects.as_deref(),
                        anchor.fallback_cfi.as_deref(),
                        value.exact_text.as_str(),
                        style,
                        value.color.as_str(),
                        value.note.as_str(),
                        created_at,
                        modified_at,
                        value.deleted.then_some(modified_at),
                    )
                    .map_err(DatabaseError::operation)?;
                    }
                }
                MutationBody::PdfReaderMetadata { content_hash, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        let checksum = sync_common::ContentHash::new(&value.checksum);
                        if !value.valid_for(checksum.as_str()) {
                            return Err(DatabaseError::operation("PDF ingestion checksum or metadata is invalid"));
                        }
                        let bytes = serde_json::to_vec(&value).map_err(DatabaseError::operation)?;
                        tx.pdf_reader_metadata_upsert(content_hash.as_str(), checksum.as_str(), &bytes)
                            .map_err(DatabaseError::operation)?;
                    }
                }
                MutationBody::BookToc { content_hash, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        let entries = value.to_vec();
                        let entry_count = i64::try_from(toc_entry_count(&entries)).map_err(|_| DatabaseError::operation("table of contents entry count exceeds SQLite integer range"))?;
                        let toc_json = serde_json::to_string(&entries).map_err(DatabaseError::operation)?;
                        tx.book_toc_upsert(content_hash.as_str(), entry_count, &toc_json)
                            .map_err(DatabaseError::operation)?;
                    }
                }
                MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are filtered before decoding"),
                MutationBody::Description { content_hash, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        let normalized = normalized_description(value);
                        tx.set_description(content_hash.as_str(), &normalized).map_err(DatabaseError::operation)?;
                        tx.mark_description_scanned(content_hash.as_str()).map_err(DatabaseError::operation)?;
                    }
                }
                MutationBody::Metadata { content_hash, value } => {
                    let mut purged = false;
                    tx.book_is_purged_select(content_hash.as_str(), |row| {
                        purged = row.get(0)?;
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    if !purged {
                        tx.first_book_insert(
                            content_hash.as_str(),
                            storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                        )
                        .map_err(DatabaseError::operation)?;
                        pending_metadata = Some((content_hash, value));
                    }
                }
            }
            if let Some((meta_hash, meta)) = pending_metadata {
                pending.push((meta_hash, meta, None));
            }
            tx.record_sync_state_version_insert(
                state_kind,
                state_key,
                state_subkey,
                storage_i64(change.mutation.changed_at, "change timestamp").map_err(DatabaseError::operation)?,
                i64::from(change.mutation.conflict_rank),
                &change.replica_id.to_string(),
                storage_i64(change.mutation.replica_seq.get(), "actor sequence").map_err(DatabaseError::operation)?,
                &change.mutation.mutation_id.to_string(),
            )
            .map_err(DatabaseError::operation)?;
            #[cfg(not(target_arch = "wasm32"))]
            match body {
                MutationBody::Placement { content_hash, .. } | MutationBody::BookLifecycle { content_hash, .. } => {
                    tx.queue_book_work_insert(content_hash.as_str()).map_err(DatabaseError::operation)?;
                }
                _ => {}
            }
            if matches!(body, MutationBody::DirectoryName { .. } | MutationBody::DirectoryParent { .. } | MutationBody::DirectoryLifecycle { .. }) {
                reproject = true;
            }
        }
        // Shared projection tail. Ordered pages arrive with an empty
        // snapshot pair and read current rows here, so same-page purges
        // and repeated identities observe page order exactly as before.
        // A book purged by its own page must not be resurrected below.
        for (meta_hash, meta, precomputed) in pending {
            let mut purged = false;
            tx.book_is_purged_select(meta_hash.as_str(), |row| {
                purged = row.get(0)?;
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            if purged {
                continue;
            }
            // Stored previous book; missing/legacy-invalid values
            // require a full projection, not a no-op. Batch pages pre-read
            // this pair while lifecycle rows are fresh; ordered pages read
            // current rows here.
            let (previous, credits_match): (Option<book_model::BookMetadata>, bool) = match precomputed {
                Some(precomputed) => precomputed,
                None => {
                    let mut raw: Option<Vec<u8>> = None;
                    tx.meta_stored_book_select(meta_hash.as_str(), |row| {
                        raw = Some(row.get::<_, Vec<u8>>(0)?);
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let previous: Option<book_model::BookMetadata> =
                        raw.and_then(|raw| decode_book_metadata(&raw).ok());
                    // Exact ordered credit equality, including stable identity,
                    // role and sort name. The pull path always preserves incoming
                    // ids, so there is no provisional-name-candidate branch here.
                    let mut stored_credits = Vec::new();
                    tx.authors_contributors_match_select(meta_hash.as_str(), |row| {
                        stored_credits.push((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Vec<u8>>(4)?,
                        ));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let mut credits_match = meta.contributors.len() == stored_credits.len();
                    for (position, credit) in meta.contributors.iter().enumerate() {
                        if parse_author_name_opt(credit.name())?.is_none() {
                            credits_match = false;
                            break;
                        }
                        if !credits_match {
                            break;
                        }
                        match stored_credits.get(position) {
                            Some((stored_position, stable_id, name, sort_name, role))
                                if *stored_position == position as i64
                                    && stable_id.as_deref() == Some(credit.agent_id().to_string().as_str())
                                    && name.as_str() == credit.name()
                                    && sort_name.as_deref() == credit.sort_name()
                                    && role.as_slice() == credit.role_code().0 => {}
                            _ => {
                                credits_match = false;
                                break;
                            }
                        }
                    }
                    (previous, credits_match)
                }
            };
            let stored_contributors: Option<Vec<(i64, book_model::Contributor)>> = if credits_match {
                None
            } else {
                let mut stored = Vec::new();
                for contributor in &meta.contributors {
                    let Some(name) = parse_author_name_opt(contributor.name())? else { continue };
                    let mut incoming: Option<(i64, String)> = None;
                    tx.author_identity_by_stable_id(&contributor.agent_id().to_string(), |row| {
                        incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let (identity_local_id, contributor_id) = match incoming {
                        Some((local_id, stable_id)) => {
                            let stable_id = book_model::AgentId::parse_str(&stable_id).map_err(|error| DatabaseError::operation(format!("stored author identity is not readable: {error}")))?;
                            (local_id, stable_id)
                        }
                        None => {
                            tx.authors_insert_author_identity(
                                &contributor.agent_id().to_string(),
                                name.as_str(),
                                name.match_key().as_str(),
                                unix_millis()?,
                            )
                            .map_err(DatabaseError::operation)?;
                            (tx.last_insert_rowid(), contributor.agent_id())
                        }
                    };
                    let reidentified = book_model::Contributor::with_id(
                        contributor_id,
                        contributor.name(),
                        contributor.sort_name().map(ToOwned::to_owned),
                        contributor.role_code(),
                    )
                    .map_err(|error| DatabaseError::operation(format!("contributor credit is not writable: {error}")))?;
                    stored.push((identity_local_id, reidentified));
                }
                Some(stored)
            };
            // One book-row update, not three successive trigger/outbox refreshes.
            if previous.as_ref() == Some(&meta.book) {
                tx.meta_before_contributor_fast_path_update(meta_hash.as_str(), &meta.title)
                    .map_err(DatabaseError::operation)?;
                if let Some(contributors) = &stored_contributors {
                    for (position, (identity_local_id, credit)) in contributors.iter().enumerate() {
                        let role = credit.role_code().0;
                        tx.authors_upsert_book_contributor(
                            meta_hash.as_str(),
                            *identity_local_id,
                            i64::try_from(position).unwrap_or(i64::MAX),
                            credit.name(),
                            credit.sort_name(),
                            &role,
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                    tx.authors_trim_book_contributors(meta_hash.as_str(), i64::try_from(contributors.len()).unwrap_or(i64::MAX))
                        .map_err(DatabaseError::operation)?;
                }
            } else {
                // Resolve local publisher identity rows at write time; no
                // classification, metadata merging or serialization is
                // repeated here.
                let mut prepared_publishers = Vec::with_capacity(meta.book.publishers.len());
                for publisher in &meta.book.publishers {
                    let Some(name) = parse_author_name_opt(publisher.name().as_str())? else {
                        return Err(DatabaseError::operation("publisher name did not produce an agent name"));
                    };
                    let mut incoming: Option<(i64, String)> = None;
                    tx.author_identity_by_stable_id(&publisher.agent_id().to_string(), |row| {
                        incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                        Ok(())
                    })
                    .map_err(DatabaseError::operation)?;
                    let (_, stable_id) = match incoming {
                        Some((local_id, stable_id)) => {
                            let stable_id = book_model::AgentId::parse_str(&stable_id).map_err(|error| DatabaseError::operation(format!("stored author identity is not readable: {error}")))?;
                            (local_id, stable_id)
                        }
                        None => {
                            tx.authors_insert_author_identity(
                                &publisher.agent_id().to_string(),
                                name.as_str(),
                                name.match_key().as_str(),
                                unix_millis()?,
                            )
                            .map_err(DatabaseError::operation)?;
                            (tx.last_insert_rowid(), publisher.agent_id())
                        }
                    };
                    prepared_publishers.push(
                        book_model::PublisherCredit::with_id(stable_id, publisher.name().as_str())
                            .map_err(|error| DatabaseError::operation(format!("publisher credit is not writable: {error}")))?,
                    );
                }
                let mut prepared_book = meta.book.clone();
                prepared_book.publishers = prepared_publishers;
                let encoded = sync_common::wire::encode(&prepared_book).map_err(DatabaseError::operation)?;
                tx.meta_update_book_metadata_state_preserving_ids_update_2(meta_hash.as_str(), &meta.title, &encoded)
                    .map_err(DatabaseError::operation)?;
                if let Some(contributors) = &stored_contributors {
                    for (position, (identity_local_id, credit)) in contributors.iter().enumerate() {
                        let role = credit.role_code().0;
                        tx.authors_upsert_book_contributor(
                            meta_hash.as_str(),
                            *identity_local_id,
                            i64::try_from(position).unwrap_or(i64::MAX),
                            credit.name(),
                            credit.sort_name(),
                            &role,
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                    tx.authors_trim_book_contributors(meta_hash.as_str(), i64::try_from(contributors.len()).unwrap_or(i64::MAX))
                        .map_err(DatabaseError::operation)?;
                }
                let projected = project_book(&prepared_book, encoded, previous.as_ref())?;
                let mut book_id: Option<i64> = None;
                tx.meta_replace_prepared_projections_select(meta_hash.as_str(), |r| {
                    book_id = r.get(0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let book_id = book_id.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
                let mut identity_agents: std::collections::HashMap<book_model::AgentId, i64> = std::collections::HashMap::new();
                let mut identity_publishers: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
                if previous.as_ref().is_none_or(|old| old.publishers != projected.metadata.publishers) {
                    tx.clear_book_publishers(book_id).map_err(DatabaseError::operation)?;
                    for (position, publisher) in projected.metadata.publishers.iter().enumerate() {
                        let name = catalog_model::AuthorName::parse(publisher.name().as_str())
                            .map_err(|error| DatabaseError::operation(format!("publisher name is not readable: {error}")))?;
                        let agent = if let Some(row) = identity_agents.get(&publisher.agent_id()) {
                            *row
                        } else {
                            let mut incoming: Option<(i64, String)> = None;
                            tx.author_identity_by_stable_id(&publisher.agent_id().to_string(), |row| {
                                incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                                Ok(())
                            })
                            .map_err(DatabaseError::operation)?;
                            let (row, _) = match incoming {
                                Some((local_id, _)) => (local_id, publisher.agent_id()),
                                None => {
                                    tx.authors_insert_author_identity(
                                        &publisher.agent_id().to_string(),
                                        name.as_str(),
                                        name.match_key().as_str(),
                                        unix_millis()?,
                                    )
                                    .map_err(DatabaseError::operation)?;
                                    (tx.last_insert_rowid(), publisher.agent_id())
                                }
                            };
                            identity_agents.insert(publisher.agent_id(), row);
                            row
                        };
                        let key = publisher.name().match_key().as_str().to_owned();
                        let row = if let Some(row) = identity_publishers.get(&key) {
                            *row
                        } else {
                            tx.insert_publisher(publisher.name().as_str(), &key)
                                .map_err(DatabaseError::operation)?;
                            let mut row: Option<i64> = None;
                            tx.authors_publisher_select(&key, |r| {
                                row = r.get(0)?;
                                Ok(())
                            })
                            .map_err(DatabaseError::operation)?;
                            let row = row.ok_or(rusqlite::Error::QueryReturnedNoRows).map_err(DatabaseError::operation)?;
                            identity_publishers.insert(key, row);
                            row
                        };
                        tx.insert_book_publisher(book_id, row, agent, position as i64, publisher.name().as_str())
                            .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.identifiers != projected.metadata.identifiers) {
                    tx.clear_book_identifiers(book_id).map_err(DatabaseError::operation)?;
                    for (position, identifier) in projected.metadata.identifiers.iter().enumerate() {
                        tx.insert_book_identifier(
                            book_id,
                            position as i64,
                            &identifier_scheme_label(identifier.scheme()),
                            identifier.value(),
                            identifier.canonical_value().as_deref(),
                            identifier.source(),
                            identifier_scope_label(identifier.scope()),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.languages != projected.metadata.languages) {
                    tx.clear_book_languages(book_id).map_err(DatabaseError::operation)?;
                    for (position, language) in projected.metadata.languages.iter().enumerate() {
                        tx.insert_book_language(book_id, position as i64, language.as_str())
                            .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.subjects != projected.metadata.subjects) {
                    for assignment in &projected.assignments {
                        if assignment.system_id() != subject_projection::BISAC_SYSTEM_ID {
                            let components: Vec<&str> = assignment.subject_path().split(" / ").map(str::trim).filter(|component| !component.is_empty()).collect();
                            tx.meta_ensure_external_subject_nodes_insert(assignment.system_id())
                                .map_err(DatabaseError::operation)?;
                            let mut path = String::new();
                            let mut parent_path = String::new();
                            for component in components {
                                if !path.is_empty() {
                                    path.push_str(" / ");
                                }
                                path.push_str(component);
                                tx.meta_ensure_external_subject_nodes_insert_2(assignment.system_id(), &path, &parent_path, component)
                                    .map_err(DatabaseError::operation)?;
                                parent_path.clone_from(&path);
                            }
                            tx.meta_ensure_external_subject_nodes_update(assignment.system_id(), assignment.matcher_version())
                                .map_err(DatabaseError::operation)?;
                        }
                    }
                    for (index, concept, path) in &projected.concepts {
                        let code = &projected.subject_codes[*index];
                        let version = source_subject_version(code.system_id());
                        tx.apply_insert(code.system_id(), version, code.code())
                            .map_err(DatabaseError::operation)?;
                        tx.apply_insert_2(code.system_id(), version, code.code(), *concept, source_mapping_type(code.system_id()), subject_projection::UNIFIED_MATCHER_VERSION)
                            .map_err(DatabaseError::operation)?;
                        tx.apply_insert_3(code.system_id(), code.code(), path, subject_projection::UNIFIED_MATCHER_VERSION)
                            .map_err(DatabaseError::operation)?;
                    }
                    for rows in &projected.classification_rows {
                        let (select_sql, delete_sql, insert_sql): (&str, &str, &str) = match rows.table {
                            "book_subject" => (
                                include_str!("reconcile/book_subject_select.sql"),
                                include_str!("reconcile/book_subject_delete.sql"),
                                include_str!("reconcile/book_subject_insert.sql"),
                            ),
                            "book_subject_code" => (
                                include_str!("reconcile/book_subject_code_select.sql"),
                                include_str!("reconcile/book_subject_code_delete.sql"),
                                include_str!("reconcile/book_subject_code_insert.sql"),
                            ),
                            "book_subject_assignment" => (
                                include_str!("reconcile/book_subject_assignment_select.sql"),
                                include_str!("reconcile/book_subject_assignment_delete.sql"),
                                include_str!("reconcile/book_subject_assignment_insert.sql"),
                            ),
                            "book_subject_assignment_evidence" => (
                                include_str!("reconcile/book_subject_assignment_evidence_select.sql"),
                                include_str!("reconcile/book_subject_assignment_evidence_delete.sql"),
                                include_str!("reconcile/book_subject_assignment_evidence_insert.sql"),
                            ),
                            "book_unified_concept" => (
                                include_str!("reconcile/book_unified_concept_select.sql"),
                                include_str!("reconcile/book_unified_concept_delete.sql"),
                                include_str!("reconcile/book_unified_concept_insert.sql"),
                            ),
                            "book_unified_concept_evidence" => (
                                include_str!("reconcile/book_unified_concept_evidence_select.sql"),
                                include_str!("reconcile/book_unified_concept_evidence_delete.sql"),
                                include_str!("reconcile/book_unified_concept_evidence_insert.sql"),
                            ),
                            _ => return Err(DatabaseError::operation("unknown classification table")),
                        };
                        let existing: std::collections::BTreeMap<Vec<Cell>, Vec<Cell>> = tx
                            .prepare_cached(select_sql)
                            .map_err(DatabaseError::operation)?
                            .query_map([book_id], |r| {
                                (0..rows.width)
                                    .map(|i| match r.get_ref(i)? {
                                        rusqlite::types::ValueRef::Null => Ok(Cell::Null),
                                        rusqlite::types::ValueRef::Integer(v) => Ok(Cell::Integer(v)),
                                        rusqlite::types::ValueRef::Text(v) => Ok(Cell::Text(std::str::from_utf8(v).map_err(|error| invalid_text_column(i, error.to_string()))?.into())),
                                        _ => Err(invalid_text_column(i, "unexpected classification value")),
                                    })
                                    .collect::<rusqlite::Result<Vec<_>>>()
                            })
                            .map_err(DatabaseError::operation)?
                            .collect::<Result<Vec<_>, rusqlite::Error>>()
                            .map_err(DatabaseError::operation)?
                            .into_iter()
                            .map(|v| (v[..rows.keys].to_vec(), v))
                            .collect();
                        for key in existing.keys().filter(|key| !rows.desired.contains_key(*key)) {
                            tx.prepare_cached(delete_sql)
                                .map_err(DatabaseError::operation)?
                                .execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(key.iter().cloned())))
                                .map_err(DatabaseError::operation)?;
                        }
                        for (key, values) in &rows.desired {
                            if existing.get(key) == Some(values) {
                                continue;
                            }
                            tx.prepare_cached(insert_sql)
                                .map_err(DatabaseError::operation)?
                                .execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(values.iter().cloned())))
                                .map_err(DatabaseError::operation)?;
                        }
                    }
                }
                if previous.as_ref().is_none_or(|old| old.collections != projected.metadata.collections) {
                    tx.clear_book_collections(book_id).map_err(DatabaseError::operation)?;
                    for (position, collection) in projected.metadata.collections.iter().enumerate() {
                        tx.insert_book_collection(
                            book_id,
                            position as i64,
                            collection.name(),
                            collection.collection_type(),
                            collection.position(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                if previous.as_ref().is_none_or(|old| old.dates != projected.metadata.dates) {
                    tx.clear_book_dates(book_id).map_err(DatabaseError::operation)?;
                    for (position, date) in projected.metadata.dates.iter().enumerate() {
                        tx.insert_book_date(
                            book_id,
                            position as i64,
                            date.value(),
                            None::<i32>,
                            None::<i64>,
                            None::<i64>,
                            date.event(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
                let mut ordered_titles = projected.metadata.alternate_titles.iter().collect::<Vec<_>>();
                ordered_titles.sort_by_key(|title| title.source() == "title:colon-fallback" && title.title_type() == Some("subtitle"));
                let alternate_titles = ordered_titles
                    .into_iter()
                    .map(|value| (value.value().to_owned(), value.title_type().map(ToOwned::to_owned), value.sort_as().map(ToOwned::to_owned)))
                    .collect::<Vec<_>>();
                for (kind, values) in [
                    ("alternate_title", alternate_titles),
                    ("rights", projected.metadata.rights.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("source", projected.metadata.sources.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("type", projected.metadata.types.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("format", projected.metadata.formats.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("relation", projected.metadata.relations.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    ("coverage", projected.metadata.coverage.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>()),
                    (
                        "property",
                        projected
                            .metadata
                            .properties
                            .iter()
                            .map(|property| (property.value().to_owned(), property.refines().map(ToOwned::to_owned), Some(property.property().to_owned())))
                            .collect::<Vec<_>>(),
                    ),
                    ("enrichment_isbn", projected.references.clone()),
                ] {
                    let changed = previous.as_ref().is_none_or(|old| match kind {
                        "alternate_title" => old.alternate_titles != projected.metadata.alternate_titles,
                        "rights" => old.rights != projected.metadata.rights,
                        "source" => old.sources != projected.metadata.sources,
                        "type" => old.types != projected.metadata.types,
                        "format" => old.formats != projected.metadata.formats,
                        "relation" => old.relations != projected.metadata.relations,
                        "coverage" => old.coverage != projected.metadata.coverage,
                        "property" | "enrichment_isbn" => old.properties != projected.metadata.properties,
                        _ => true,
                    });
                    if !changed {
                        continue;
                    }
                    tx.meta_replace_prepared_projections_delete(book_id, kind)
                        .map_err(DatabaseError::operation)?;
                    for (position, (value, qualifier, secondary)) in values.iter().enumerate() {
                        tx.insert_book_value(
                            book_id,
                            kind,
                            position as i64,
                            value,
                            qualifier.as_deref(),
                            secondary.as_deref(),
                        )
                        .map_err(DatabaseError::operation)?;
                    }
                }
            }
        }
        if reproject {
            // Actual parent/lifecycle columns are projected in a batch while
            // change_origin is remote, so the per-row hierarchy trigger is
            // suppressed. Rebuild the derived ownership once after the final
            // live tree is known.
            #[cfg(not(target_arch = "wasm32"))]
            let mut before: std::collections::HashMap<String, (String, String, bool)> = std::collections::HashMap::new();
            #[cfg(not(target_arch = "wasm32"))]
            tx.directory_projection_rows_select(|row| {
                before.insert(row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?));
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            let mut intents: std::collections::HashMap<sync_common::DirId, library_replica::DirectoryIntent> = std::collections::HashMap::new();
            let mut rows: Vec<(String, String, String, i64)> = Vec::new();
            tx.get_remote_directory_intents(|row| {
                rows.push((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?));
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            for (id_text, parent_text, name, lifecycle) in rows {
                let id = parse_uuid_column(id_text.clone(), 0).map_err(DatabaseError::operation)?;
                let parent_id = parse_uuid_column(parent_text, 1).map_err(DatabaseError::operation)?;
                let lifecycle = match lifecycle {
                    0 => DirectoryLifecycleState::Present,
                    1 => DirectoryLifecycleState::Deleted,
                    2 => DirectoryLifecycleState::Purged,
                    _ => return Err(DatabaseError::operation("invalid directory lifecycle state")),
                };
                let mut name_version: Option<library_replica::VersionKey> = None;
                tx.read_sync_state_version_select("directory_name", &id_text, "", |row| {
                    name_version = StoredVersion::decode_row(row, 0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let mut parent_version: Option<library_replica::VersionKey> = None;
                tx.read_sync_state_version_select("directory_parent", &id_text, "", |row| {
                    parent_version = StoredVersion::decode_row(row, 0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                let mut lifecycle_version: Option<library_replica::VersionKey> = None;
                tx.read_sync_state_version_select("directory_lifecycle", &id_text, "", |row| {
                    lifecycle_version = StoredVersion::decode_row(row, 0)?;
                    Ok(())
                })
                .map_err(DatabaseError::operation)?;
                if let (Some(name_version), Some(parent_version), Some(_)) = (name_version, parent_version, lifecycle_version) {
                    let version = name_version.max(parent_version);
                    intents.insert(id, DirectoryIntent { id, parent_id, name, lifecycle, version });
                }
            }
            let projection = library_replica::project_directories(&intents);
            tx.hide_remote_directories().map_err(DatabaseError::operation)?;
            let mut ids: Vec<_> = intents.keys().copied().collect();
            ids.sort();
            for id in &ids {
                let intent = &intents[id];
                let projected_parent = if projection.suppressed.contains(id) { sync_common::ROOT_DIR_ID } else { intent.parent_id };
                let projected_name = projection.names.get(id).unwrap_or(&intent.name);
                tx.project_remote_directory(&intent.id.to_string(), &projected_parent.to_string(), projected_name.as_str())
                    .map_err(DatabaseError::operation)?;
                if intent.lifecycle == DirectoryLifecycleState::Purged {
                    tx.mark_remote_directory_purged(&intent.id.to_string()).map_err(DatabaseError::operation)?;
                }
            }
            for id in projection.ordered_active {
                tx.activate_remote_directory(&id.to_string()).map_err(DatabaseError::operation)?;
            }
            tx.rebuild_folder_book_ownership().map_err(DatabaseError::operation)?;
            #[cfg(not(target_arch = "wasm32"))]
            let mut after: Vec<(String, (String, String, bool))> = Vec::new();
            #[cfg(not(target_arch = "wasm32"))]
            tx.directory_projection_rows_select(|row| {
                after.push((row.get::<_, String>(0)?, (row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?)));
                Ok(())
            })
            .map_err(DatabaseError::operation)?;
            #[cfg(not(target_arch = "wasm32"))]
            for (id, state) in after
            {
                if before.get(&id) != Some(&state) {
                    let dir = sync_common::DirId::parse_str(&id).map_err(DatabaseError::operation)?;
                    tx.queue_directory_work_insert(&dir.to_string()).map_err(DatabaseError::operation)?;
                }
            }
        }
        let changed = !page.mutations.is_empty();
        write_cursor_revision(&tx, PullCursorKey::State, page.next_cursor.state_revision)?;
        write_cursor_revision(&tx, PullCursorKey::Reading, page.next_cursor.reading_revision)?;
        if origin_remote {
            let restore = previous_origin.as_deref().unwrap_or("local");
            tx.restore_origin(restore).map_err(DatabaseError::operation)?;
        }
        tx.commit().map_err(DatabaseError::operation)?;
        Ok(changed)
    }
}

// ---- state ----
// Sync cursor, inventory, and recovery primitives.
//
// Each method is one bounded read or one bounded write in its own
// transaction. Page- and chunk-sized looping belongs to the orchestrator,
// which drives these methods and the network between them.

use sync_common::{StateCell, SyncCursor};

fn unix_millis_saturating() -> Result<i64, DatabaseError> {
    u64::try_from(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_millis(),
    )
    .map(|value| value.min(i64::MAX as u64) as i64)
    .map_err(DatabaseError::operation)
}

fn read_cell(row: &rusqlite::Row<'_>) -> rusqlite::Result<StateCell> {
    Ok(StateCell { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? })
}

fn parse_stored_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, DatabaseError> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(DatabaseError::operation)?;
            sync_common::LibraryRevision::new(raw).map_err(DatabaseError::operation)
        })
        .transpose()
}

/// Durable marker key for an in-progress cursor recovery.
pub const CURSOR_RECOVERY_PENDING_KEY: &str = "cursor_recovery_pending";

impl Database {
    /// Reads the independent state and reading pull checkpoints.
    pub fn sync_pull_cursor(&self) -> Result<SyncCursor, DatabaseError> {
        let mut raw = None;
        self.connection.sync_pull_cursor(|row| {
            raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?));
            Ok(())
        })?;
        let (state, reading) = raw.ok_or_else(|| DatabaseError::operation("sync metadata row is missing"))?;
        Ok(SyncCursor { state_revision: parse_stored_revision(state)?, reading_revision: parse_stored_revision(reading)? })
    }

    /// Whether a rejected cursor still needs recovery.
    pub fn sync_cursor_recovery_pending(&self) -> Result<bool, DatabaseError> {
        let mut pending: Option<i64> = None;
        self.connection.sync_cursor_recovery_pending(|row| {
            pending = row.get(0)?;
            Ok(())
        })?;
        Ok(pending == Some(1))
    }

    /// Resets both pull channels and persists the recovery marker in one commit.
    pub fn sync_begin_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_begin_cursor_recovery().map(|_| ()).map_err(Into::into)
    }

    /// Persists both pull checkpoints. A missing revision clears its channel.
    pub fn set_sync_pull_cursor(&self, cursor: &SyncCursor) -> Result<(), DatabaseError> {
        write_cursor_revision(&self.connection, PullCursorKey::State, cursor.state_revision)?;
        write_cursor_revision(&self.connection, PullCursorKey::Reading, cursor.reading_revision)?;
        Ok(())
    }

    /// Reads one explicit sync_metadata column as text. Unknown keys read as
    /// `None`; absent rows and null columns both read as `None`.
    pub fn sync_metadata(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        let mut value = None;
        match key {
            "replica_id" => self.connection.syncmeta_replica_id(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "change_origin" => self.connection.change_origin_select(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "last_successful_sync_at" => self.connection.syncmeta_last_successful_sync_at(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "last_pull_state_seq" => self.connection.syncmeta_last_pull_state_seq(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "last_pull_reading_seq" => self.connection.syncmeta_last_pull_reading_seq(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "cursor_recovery_pending" => self.connection.syncmeta_cursor_recovery_pending(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "scan_seq" => self.connection.syncmeta_scan_seq(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "cloud_storage_enabled" => self.connection.syncmeta_cloud_storage_enabled(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "remote_library_name" => self.connection.syncmeta_remote_library_name(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            "server_library_created" => self.connection.syncmeta_server_library_created(|row| {
                value = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?,
            _ => return Ok(None),
        };
        Ok(value.flatten())
    }

    /// Read only the next bounded set of register identities. Local changes
    /// made behind this cursor already create durable outbox work through
    /// normal triggers.
    pub fn sync_inventory_page(&self, after: Option<&StateCell>) -> Result<Vec<StateCell>, DatabaseError> {
        let (kind, key, subkey) = after.map(|cell| (cell.kind.as_str(), cell.entity_key.as_str(), cell.entity_subkey.as_str())).unwrap_or(("", "", ""));
        let mut cells = Vec::new();
        self.connection.sync_state_inventory_page(kind, key, subkey, |row| {
            cells.push(read_cell(row)?);
            Ok(())
        })?;
        Ok(cells)
    }

    pub fn sync_inventory_checkpoint(&self) -> Result<Option<StateCell>, DatabaseError> {
        let mut raw = None;
        self.connection.sync_inventory_checkpoint(|row| {
            raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?));
            Ok(())
        })?;
        Ok(raw.and_then(|(kind, key, subkey)| {
            kind.map(|kind| StateCell { kind, entity_key: key.unwrap_or_default(), entity_subkey: subkey.unwrap_or_default() })
        }))
    }

    pub fn sync_save_inventory_checkpoint(&self, after: Option<StateCell>) -> Result<(), DatabaseError> {
        if let Some(after) = after {
            self.connection.sync_save_inventory_checkpoint(&after.kind, &after.entity_key, &after.entity_subkey)?;
        } else {
            self.connection.sync_clear_inventory_checkpoint()?;
        }
        Ok(())
    }

    /// Recreates durable outbox entries for state cells confirmed absent on
    /// the server, in one transaction. Cells already queued are skipped
    /// without re-encoding; cells with no current value are skipped.
    pub fn sync_enqueue_missing_state_cells(&self, cells: &[StateCell]) -> Result<usize, DatabaseError> {
        if cells.is_empty() {
            return Ok(0);
        }
        let changed_at = unix_millis_saturating()?;
        let mut pending = Vec::new();
        for cell in cells {
            if self.outbox_identity_exists(cell)? {
                continue;
            }
            let Some((body, conflict_rank)) = self.read_current_state_body(cell)? else { continue };
            pending.push((cell, body, conflict_rank));
        }
        if pending.is_empty() {
            return Ok(0);
        }
        let transaction = self.connection.unchecked_transaction()?;
        let mut inserted = 0;
        for (cell, body, conflict_rank) in &pending {
            inserted += transaction.sync_enqueue_outbox_cell(&cell.kind, &cell.entity_key, &cell.entity_subkey, body, changed_at, *conflict_rank)?;
        }
        transaction.commit()?;
        Ok(inserted as usize)
    }

    fn outbox_identity_exists(&self, cell: &StateCell) -> Result<bool, DatabaseError> {
        let mut exists = false;
        self.connection.sync_outbox_identity_exists(&cell.kind, &cell.entity_key, &cell.entity_subkey, |row| {
            exists = row.get(0)?;
            Ok(())
        })?;
        Ok(exists)
    }

    /// Recovery commits one page at a time while retaining its durable
    /// marker. After interruption, restarting from the first page is safe
    /// because enqueue preserves existing outbox identities. Only an empty
    /// final page clears the marker.
    pub fn sync_recover_state_page(&self, after: Option<&StateCell>) -> Result<(Option<StateCell>, usize), DatabaseError> {
        let cells = self.sync_inventory_page(after)?;
        if cells.is_empty() {
            self.clear_cursor_recovery()?;
            return Ok((None, 0));
        }
        let changed_at = unix_millis_saturating()?;
        let mut pending = Vec::new();
        for cell in &cells {
            if self.outbox_identity_exists(cell)? {
                continue;
            }
            let Some((body, conflict_rank)) = self.read_current_state_body(cell)? else { continue };
            pending.push((cell, body, conflict_rank));
        }
        if !pending.is_empty() {
            let transaction = self.connection.unchecked_transaction()?;
            for (cell, body, conflict_rank) in &pending {
                transaction.sync_enqueue_outbox_cell(&cell.kind, &cell.entity_key, &cell.entity_subkey, body, changed_at, *conflict_rank)?;
            }
            transaction.commit()?;
        }
        let inserted = pending.len();
        Ok((cells.last().cloned(), inserted))
    }

    fn clear_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_clear_cursor_recovery().map(|_| ()).map_err(Into::into)
    }

    fn read_current_state_body(&self, cell: &StateCell) -> Result<Option<(Vec<u8>, i64)>, DatabaseError> {
        use rusqlite::OptionalExtension as _;
        fn single_pair(
            query: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>,
        ) -> Result<Option<(Vec<u8>, i64)>, DatabaseError> {
            let mut found = None;
            query(&mut |row| {
                found = Some((row.get(0)?, row.get(1)?));
                Ok(())
            })
            .optional()?;
            Ok(found)
        }
        let key = cell.entity_key.as_str();
        let subkey = cell.entity_subkey.as_str();
        match cell.kind.as_str() {
            "directory_name" => single_pair(|read| self.connection.sync_current_directory_name(key, read)),
            "directory_parent" => single_pair(|read| self.connection.sync_current_directory_parent(key, read)),
            "directory_lifecycle" => single_pair(|read| self.connection.sync_current_directory_lifecycle(key, read)),
            "book_lifecycle" => {
                let mut lifecycle: Option<i64> = None;
                self.connection
                    .sync_current_book_lifecycle_state(key, |row| {
                        lifecycle = row.get(0)?;
                        Ok(())
                    })
                    .optional()?;
                match lifecycle {
                    Some(2) => single_pair(|read| self.connection.sync_current_book_purged(key, read)),
                    Some(1) => single_pair(|read| self.connection.sync_current_book_deleted(key, read)),
                    Some(0) => single_pair(|read| self.connection.sync_current_book_added(key, read)),
                    Some(_) | None => Ok(None),
                }
            }
            "placement" => single_pair(|read| self.connection.sync_current_placement(key, subkey, read)),
            "reading_position" => single_pair(|read| self.connection.sync_current_reading_position(key, read)),
            "description" => single_pair(|read| self.connection.sync_current_description(key, read)),
            "metadata" => single_pair(|read| self.connection.sync_current_metadata(key, read)),
            "annotation" => single_pair(|read| self.connection.sync_current_annotation(key, read)),
            _ => Ok(None),
        }
    }
}

include_sql!("src/sync/sync_state.sql");

#[cfg(test)]
mod state_tests {
    use crate::*;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-state-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn recovery_flag_lifecycle() {
        let holder = open_test_database();
        let database = &holder.database;
        assert!(!database.sync_cursor_recovery_pending().expect("read flag"));
        database.sync_begin_cursor_recovery().expect("begin recovery");
        assert!(database.sync_cursor_recovery_pending().expect("reread flag"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), SyncCursor::default());
        // No version rows exist, so the first page is also the last and clears
        // the marker.
        assert_eq!(database.sync_recover_state_page(None).expect("recover page"), (None, 0));
        assert!(!database.sync_cursor_recovery_pending().expect("cleared flag"));
    }

    #[test]
    fn inventory_checkpoint_round_trip() {
        let holder = open_test_database();
        let database = &holder.database;
        assert_eq!(database.sync_inventory_checkpoint().expect("read checkpoint"), None);
        let cell =
            StateCell { kind: "directory_name".to_owned(), entity_key: sync_common::ROOT_DIR_ID.to_string(), entity_subkey: String::new() };
        database.sync_save_inventory_checkpoint(Some(cell.clone())).expect("save checkpoint");
        assert_eq!(database.sync_inventory_checkpoint().expect("reread checkpoint"), Some(cell));
        database.sync_save_inventory_checkpoint(None).expect("clear checkpoint");
        assert_eq!(database.sync_inventory_checkpoint().expect("cleared checkpoint"), None);
    }

    #[test]
    fn missing_cells_reencode_current_state() {
        let holder = open_test_database();
        let database = &holder.database;
        let created = database
            .create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Repair seed".to_owned())
            .expect("seed directory");
        let ids: Vec<sync_common::MutationId> =
            database.sync_publishable_mutations().expect("read snapshot").iter().map(|mutation| mutation.mutation_id).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        let cell = StateCell { kind: "directory_name".to_owned(), entity_key: created.id.to_string(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[cell]).expect("enqueue"), 1);
        // Already queued cells are skipped without re-encoding.
        let snapshot = database.sync_publishable_mutations().expect("reread snapshot");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].body, library_replica::MutationBody::DirectoryName { .. }));
        let duplicate = StateCell {
            kind: "directory_name".to_owned(),
            entity_key: created.id.to_string(),
            entity_subkey: String::new(),
        };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[duplicate]).expect("re-enqueue"), 0);
        // Unknown kinds have no current value and are skipped.
        let unknown = StateCell { kind: "no_such_kind".to_owned(), entity_key: "x".to_owned(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[unknown]).expect("enqueue unknown"), 0);
    }
}

// ---- outbox ----
// Sync outbox snapshots and acknowledgements.
//
// The orchestrator reads a publishable snapshot, performs its network work
// with no database reference held, then commits acknowledgements back in
// caller-sized batches. One method call is one transaction.

use library_replica::{ StateMutation};
use sync_common::{ ReplicaSeq};

#[derive(Clone, Debug)]
struct OutboxRow {
    mutation_id: String,
    state_kind: String,
    state_key: String,
    body: Vec<u8>,
    changed_at: i64,
    replica_seq: i64,
    state_subkey: String,
}

fn decode_outbox_row(row: OutboxRow) -> Result<StateMutation, DatabaseError> {
    let corrupt = |reason: &'static str| {
        DatabaseError::operation(format!(
            "corrupt local sync mutation at replica sequence {}: {reason}",
            row.replica_seq
        ))
    };
    if row.state_kind == "catalog_book" {
        return Err(corrupt("catalog-book mutations are no longer supported"));
    }
    let mutation_id = MutationId::parse(&row.mutation_id).map_err(|_| corrupt("invalid mutation id"))?;
    let body: MutationBody = sync_common::wire::decode(&row.body, sync_common::wire::MAX_DECODED_REQUEST_BYTES)
        .map_err(|_| corrupt("body is not a canonical mutation"))?;
    let (kind, key, subkey) = canonical_outbox_key(&body);
    if row.state_kind != kind || row.state_key != key || row.state_subkey != subkey {
        return Err(corrupt("stored coalescing identity does not match the mutation body"));
    }
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let replica_seq =
        ReplicaSeq::new(u64::try_from(row.replica_seq).map_err(|_| corrupt("negative replica sequence"))?)
            .map_err(|_| corrupt("replica sequence must be positive"))?;
    Ok(StateMutation { mutation_id, body, changed_at, replica_seq })
}

fn canonical_outbox_key(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are rejected before decoding"),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
        MutationBody::PdfReaderMetadata { content_hash, value } => {
            ("pdf_reader_metadata", content_hash.to_string(), value.checksum.clone())
        }
        MutationBody::Description { content_hash, .. } => ("description", content_hash.to_string(), String::new()),
        MutationBody::BookToc { content_hash, .. } => ("book_toc", content_hash.to_string(), String::new()),
    }
}

/// Pending state stays durable while its exact local book version is
/// uploading (including rejected/retryable jobs). Only completion releases it.
fn book_ready(mutation: &StateMutation, pending: &std::collections::HashSet<String>) -> bool {
    mutation.body.upload_dependency().is_none_or(|hash| !pending.contains(hash.as_str()))
}

impl Database {
    /// Reads every outbox mutation whose book dependencies are
    /// satisfied. The returned snapshot is plain data: the caller performs
    /// network work with no database reference held, then commits
    /// acknowledgements with [`Database::sync_acknowledge_mutations`].
    pub fn sync_publishable_mutations(&self) -> Result<Vec<StateMutation>, DatabaseError> {
        let connection = &self.connection;
        let mut cloud_storage_enabled: Option<i64> = None;
        connection.sync_cloud_storage_enabled(|row| {
            cloud_storage_enabled = row.get(0)?;
            Ok(())
        })?;
        let pending: std::collections::HashSet<String> = if cloud_storage_enabled == Some(0) {
            std::collections::HashSet::new()
        } else {
            let mut pending = std::collections::HashSet::new();
            connection.sync_pending_uploads(|row| {
                pending.insert(row.get::<_, String>(0)?);
                Ok(())
            })?;
            pending
        };
        let mut rows = Vec::new();
        connection.sync_outbox_rows(|row| {
            rows.push(OutboxRow {
                mutation_id: row.get(0)?,
                state_kind: row.get(1)?,
                state_key: row.get(2)?,
                body: row.get(3)?,
                changed_at: row.get(4)?,
                replica_seq: row.get(5)?,
                state_subkey: row.get(6)?,
            });
            Ok(())
        })?;
        let mut mutations = Vec::with_capacity(rows.len());
        for row in rows {
            let mutation = decode_outbox_row(row)?;
            if book_ready(&mutation, &pending) {
                mutations.push(mutation);
            }
        }
        Ok(mutations)
    }

    /// Enqueues one reading-position change directly into the durable outbox.
    pub fn enqueue_outbox_reading_change(&self, mutation_id: &str, state_key: &str, body: &[u8], changed_at: i64) -> Result<(), DatabaseError> {
        self.connection.enqueue_outbox_reading_change(mutation_id, state_key, body, changed_at)?;
        Ok(())
    }

    /// Drops every durable outbox entry.
    pub fn clear_sync_outbox(&self) -> Result<(), DatabaseError> {
        self.connection.clear_sync_outbox()?;
        Ok(())
    }

    /// Seeds one outbox entry with an explicit replica sequence, e.g. a
    /// poison mutation the server will permanently reject.
    pub fn seed_poison_outbox_mutation(&self, replica_seq: i64, mutation_id: &str, body: &[u8], changed_at: i64) -> Result<(), DatabaseError> {
        self.connection.seed_poison_outbox_mutation(replica_seq, mutation_id, body, changed_at)?;
        Ok(())
    }

    /// Acknowledges exactly these mutation ids in one transaction. Callers
    /// batch large id sets into repeated calls; batching policy belongs to
    /// the orchestrator, not to storage.
    pub fn sync_acknowledge_mutations(&self, ids: &[MutationId]) -> Result<(), DatabaseError> {
        if ids.is_empty() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.sync_acknowledge_mutations(&payload)?;
        transaction.commit()?;
        Ok(())
    }
}

include_sql!("src/sync/sync_outbox.sql");

#[cfg(test)]
mod outbox_tests {
    use crate::*;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-outbox-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn publishable_snapshot_round_trips_through_acknowledgement() {
        let holder = open_test_database();
        let database = &holder.database;
        database
            .create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Ack seed".to_owned())
            .expect("seed directory");
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 3);
        let ids: Vec<MutationId> = snapshot.iter().map(|mutation| mutation.mutation_id).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
        database.sync_acknowledge_mutations(&[]).expect("empty acknowledgement is a no-op");
    }

    #[test]
    fn acknowledgement_commits_caller_sized_batches() {
        let holder = open_test_database();
        let database = &holder.database;
        for index in 0..90 {
            database
                .create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &format!("Batch {index}"))
                .expect("seed directory");
        }
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 270);
        let ids: Vec<MutationId> = snapshot.iter().map(|mutation| mutation.mutation_id).collect();
        for chunk in ids.chunks(256) {
            database.sync_acknowledge_mutations(chunk).expect("acknowledge chunk");
        }
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
    }
}

// ---- functions ----
use book_model::AgentId;
use book_model::AnnotationAnchor;
use book_model::AnnotationState;
use book_model::AnnotationStyle;
use book_model::BookFormat;
use library_replica::BookLifecycleState;
use book_model::Contributor;
use library_replica::DirectoryLifecycleState;
use book_model::PdfAnnotationAnchor;
use book_model::PdfAnnotationRect;
use book_model::BookMetadata;
use library_replica::ReadingPositionState;
use library_replica::SyncBookMetadata;
use library_replica::MutationBody;
use rusqlite::functions::Context as FunctionContext;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use sync_common::ContentHash as SyncContentHash;
use sync_common::UnixMillis;
use uuid::Uuid;

pub(crate) fn function_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

pub(crate) fn function_timestamp(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<UnixMillis> {
    let value = context.get::<i64>(index)?;
    u64::try_from(value).map_err(|_| function_error("negative mutation timestamp"))
}

pub(crate) fn function_book_format(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<BookFormat> {
    let raw = context.get::<String>(index)?;
    BookFormat::from_extension(&raw).ok_or_else(|| function_error(format!("book format '{raw}' is not a supported book format")))
}

pub(crate) fn function_content_hash(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<SyncContentHash> {
    Ok(SyncContentHash::new(&context.get::<String>(index)?))
}

pub(crate) fn function_contributors(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<Vec<Contributor>> {
    let bytes = context.get::<Vec<u8>>(index)?;
    sync_common::wire::decode(&bytes, 64 * 1024).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn contributors_for_book(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
    let book_row_id = context.get::<i64>(0)?;
    // SAFETY: `contributors_for_book` is only invoked from within triggers
    // firing on the same connection that owns this function registration;
    // sqlite guarantees the context's db handle is that connection while the
    // triggering statement is executing.
    let connection = unsafe { context.get_connection()? };
    let mut contributors = Vec::new();
    (&*connection).function_contributors(book_row_id, |row| {
        let author_identity_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = book_model::MarcRelatorCode(role_array);
        let mut stable_id = None;
        (&*connection).function_author_stable_id(author_identity_id, |identity_row| { stable_id = Some(identity_row.get::<_, String>(0)?); Ok(()) })?;
        let stable_id = stable_id.ok_or_else(|| function_error("contributor author identity disappeared"))?;
        let contributor_id = AgentId::parse_str(&stable_id).map_err(|error| function_error(error.to_string()))?;
        contributors.push(Contributor::with_id(contributor_id, name, sort_name, role).map_err(|error| function_error(error.to_string()))?);
        Ok(())
    })?;
    sync_common::wire::encode(&contributors).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn function_book(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<BookMetadata> {
    let bytes = context.get::<Vec<u8>>(index)?;
    if bytes.is_empty() {
        return Ok(BookMetadata::default());
    }
    sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn function_annotation_style(value: &str) -> rusqlite::Result<AnnotationStyle> {
    match value {
        "highlight" => Ok(AnnotationStyle::Highlight),
        "underline" => Ok(AnnotationStyle::Underline),
        "squiggly" => Ok(AnnotationStyle::Squiggly),
        "strikethrough" => Ok(AnnotationStyle::Strikethrough),
        _ => Err(function_error("invalid annotation style")),
    }
}

pub(crate) fn encode_trigger_mutation(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
    let kind = context.get::<String>(0)?;
    let body = match kind.as_str() {
        "directory_name" if context.len() == 3 => {
            let dir_id = function_uuid(context, 1)?;
            let name = context.get::<String>(2)?;
            MutationBody::DirectoryName { dir_id, name }
        }
        "directory_parent" if context.len() == 3 => {
            let dir_id = function_uuid(context, 1)?;
            let parent_id = function_uuid(context, 2)?;
            MutationBody::DirectoryParent { dir_id, parent_id }
        }
        "directory_lifecycle" if context.len() == 3 => {
            let dir_id = function_uuid(context, 1)?;
            let value = match context.get::<i64>(2)? {
                0 => DirectoryLifecycleState::Present,
                1 => DirectoryLifecycleState::Deleted,
                2 => DirectoryLifecycleState::Purged,
                _ => return Err(function_error("invalid directory lifecycle state")),
            };
            MutationBody::DirectoryLifecycle { dir_id, value }
        }
        "book_lifecycle" if matches!(context.len(), 9 | 10) => {
            let offset = usize::from(context.len() == 10);
            let content_hash = function_content_hash(context, 1)?;
            let value = match context.get::<i64>(2)? {
                0 => BookLifecycleState::Present {
                    metadata: SyncBookMetadata { title: context.get(3)?, contributors: function_contributors(context, 4)?, book: function_book(context, 5 + offset)? },
                    added_at: function_timestamp(context, 6 + offset)?,
                    // A book is only ever added by the scanner, which refuses to
                    // index a file whose extension it cannot classify, so a live
                    // row always carries a format. There is deliberately no
                    // fallback: an unnamed format would silently become a wrong
                    // filename on every device that downloads the book.
                    format: function_book_format(context, 7 + offset)?,
                },
                1 => BookLifecycleState::Deleted { origin_folder_id: context.get(8 + offset)? },
                2 => BookLifecycleState::Purged,
                _ => return Err(function_error("invalid book lifecycle state")),
            };
            MutationBody::BookLifecycle { content_hash, value }
        }
        "metadata" if matches!(context.len(), 5 | 6) => MutationBody::Metadata {
            content_hash: function_content_hash(context, 1)?,
            value: SyncBookMetadata { title: context.get(2)?, contributors: function_contributors(context, 3)?, book: function_book(context, context.len() - 1)? },
        },
        "pdf_reader_metadata" if context.len() == 3 => {
            let bytes: Vec<u8> = context.get(2)?;
            let value = serde_json::from_slice(&bytes).map_err(|_| function_error("invalid PDF reader metadata"))?;
            MutationBody::PdfReaderMetadata { content_hash: function_content_hash(context, 1)?, value }
        }
        "book_toc" if context.len() == 3 => {
            let toc_json: String = context.get(2)?;
            let value = serde_json::from_str(&toc_json).map_err(|_| function_error("invalid navigation document"))?;
            MutationBody::BookToc { content_hash: function_content_hash(context, 1)?, value }
        }
        "description" if context.len() == 3 => MutationBody::Description { content_hash: function_content_hash(context, 1)?, value: context.get(2)? },
        "placement" if context.len() == 5 => {
            let dir_id = function_uuid(context, 1)?;
            let content_hash = function_content_hash(context, 2)?;
            let present = context.get(3)?;
            MutationBody::Placement { dir_id, content_hash, present, origin_folder_id: context.get(4)? }
        }
        "reading_position" if context.len() == 4 => MutationBody::ReadingPosition {
            content_hash: function_content_hash(context, 1)?,
            value: ReadingPositionState { location: book_model::ReadingPosition::parse(&context.get::<String>(2)?).map_err(|error| function_error(error.to_string()))?, progress: context.get(3)? },
        },
        "annotation" if context.len() == 15 => {
            let anchor_kind = context.get::<String>(3)?;
            let anchor = match anchor_kind.as_str() {
                "epub_cfi" => AnnotationAnchor::epub_cfi(context.get::<String>(4)?),
                "pdf" => {
                    let page_index = u32::try_from(context.get::<i64>(5)?).map_err(|_| function_error("invalid PDF annotation page"))?;
                    let encoded_rects = context.get::<Vec<u8>>(6)?;
                    let rects: Vec<PdfAnnotationRect> = sync_common::wire::decode(&encoded_rects, 1024 * 1024).map_err(|error| function_error(error.to_string()))?;
                    let fallback_cfi = context.get::<Option<String>>(7)?;
                    let pdf = PdfAnnotationAnchor::new(page_index, rects, fallback_cfi);
                    AnnotationAnchor::pdf(pdf)
                }
                _ => return Err(function_error("invalid annotation anchor kind")),
            };
            MutationBody::Annotation {
                annotation_id: context.get(1)?,
                value: AnnotationState {
                    content_hash: function_content_hash(context, 2)?,
                    anchor,
                    exact_text: context.get(8)?,
                    style: function_annotation_style(&context.get::<String>(9)?)?,
                    color: context.get(10)?,
                    note: context.get(11)?,
                    created_at: function_timestamp(context, 12)?,
                    modified_at: function_timestamp(context, 13)?,
                    deleted: context.get(14)?,
                },
            }
        }
        _ => return Err(function_error(format!("invalid {kind} trigger mutation arguments"))),
    };
    sync_common::wire::encode(&body).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn register_sync_functions(conn: &Connection) -> rusqlite::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let metadata_batch_active = AtomicBool::new(false);
    conn.create_scalar_function("metadata_batch_flag", 1, FunctionFlags::SQLITE_UTF8, move |context| {
        Ok(match context.get::<Option<bool>>(0)? {
            Some(value) => metadata_batch_active.swap(value, Ordering::Relaxed),
            None => metadata_batch_active.load(Ordering::Relaxed),
        })
    })?;
    conn.create_scalar_function("bokheim_mutation", -1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, encode_trigger_mutation)?;
    conn.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(catalog_model::normalize_search_text(&context.get::<String>(0)?)))?;
    conn.create_scalar_function("portable_name_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_replica::portable_name_key(&context.get::<String>(0)?)))?;
    // Not SQLITE_DETERMINISTIC: this function re-enters the connection to
    // query live book_contributor state, so its result depends on database
    // contents, not solely its arguments.
    conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, |context| contributors_for_book(context))
}

pub(crate) fn function_uuid(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let value = context.get::<String>(index)?;
    Uuid::parse_str(&value).map_err(|error| function_error(error.to_string()))
}

include_sql!("src/sync/sync_functions.sql");
