// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]

use crate::RelativeDirPath;
use crate::catalog::{CatalogPublicationIdentity, CatalogSourceId};
use crate::library::Dir;
use crate::sync::transfer::TransferOrigin;
use crate::{BlobKind, BookPlacement, ContentHash, RelativeBookPath};
use crate::{EpubCfi, ReadProgress};
pub(crate) use app_contract::library::BookCardRow;
use include_sqlite_sql::{impl_sql, include_sql};
#[cfg(test)]
use library_model::AuthorAuthority;
use library_model::AuthorName;
use library_model::BookFormat;
#[cfg(test)]
use library_model::MarcRelatorCode;
use library_model::{
    AgentId, AuthorId, BISAC_MATCHER_VERSION, Contributor, DDC_MATCHER_VERSION, DDC_SYSTEM_ID, DDC_VERSION, ExternalAuthorId, LCC_MATCHER_VERSION, LCC_SYSTEM_ID, LCC_VERSION, PublicationIdentifierScheme, PublicationMetadata,
    PublicationSubject, PublisherCredit, UNIFIED_MATCHER_VERSION, UNIFIED_SYSTEM_ID, bisac_subject_system, ddc_subject_path, lcc_subject_path,
};
use library_model::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookLifecycleState, BookLinkState, BookMetadataState, BookmarkState, DirectoryLifecycleState, PdfAnnotationAnchor, PdfAnnotationRect, ReadingPositionState};
use library_replica::{MutationBody, StateMutation, VersionKey, catalog_publication_state_subkey};
use rusqlite::functions::{Context as FunctionContext, FunctionFlags};
use rusqlite::types::Type;
use rusqlite::{Connection, Transaction};
#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sync_common::{ContentHash as SyncContentHash, DirId, FileName, MutationId, ReplicaId, ReplicaSeq, ServerMutation, UnixMillis};
use taxonomy_contract::{TaxonomyCode, TaxonomySystem};
use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;

include_sql!("src/library/database/sql/create_library.sql");
include_sql!("src/library/database/sql/assets.sql");
include_sql!("src/library/database/sql/audiobooks.sql");
include_sql!("src/library/database/sql/authors.sql");
include_sql!("src/library/database/sql/book_access.sql");
include_sql!("src/library/database/sql/books.sql");
include_sql!("src/library/database/sql/catalog.sql");
include_sql!("src/library/database/sql/metadata.sql");
include_sql!("src/library/database/sql/browsing.sql");
include_sql!("src/library/database/sql/placements.sql");
include_sql!("src/library/database/sql/remote_reducer.sql");
include_sql!("src/library/database/sql/scanner.sql");
include_sql!("src/library/database/sql/sync.sql");
include_sql!("src/library/database/sql/views.sql");

// Version 15 uses the full content hash as the portable book identity and
// uses integer row IDs for every local relationship. Pre-15 databases are
// intentionally rebuilt by LibraryBackend::open; there is no compatibility
// representation or mixed-key migration.
pub(crate) const LIBRARY_SCHEMA_VERSION: i64 = 21;

pub(crate) fn directory_exists(conn: &Connection, directory_id: &str) -> rusqlite::Result<bool> {
    let mut exists = false;
    conn.live_directory_exists(directory_id, |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}

mod external_metadata;
mod links;
mod marks;
mod remote_reducer;

use assets::materialize_dir_components_with_markers;
use books::read_book_card_row;

pub(crate) use external_metadata::{
    AuthorEnrichmentCandidate, EditionIdentityCandidate, RichEnrichmentCandidate, apply_alternative_edition_isbns, apply_author_enrichment, apply_edition_identity, apply_external_subjects, apply_filename_isbn,
    apply_manual_edition_identity, apply_reviewed_edition_identity, apply_rich_enrichment, author_enrichment_candidates, edition_identity_candidate, edition_identity_candidates, edition_identity_reviews, record_external_metadata_attempt,
    record_external_metadata_failures, reject_edition_identity_candidates, rich_enrichment_candidates, subject_enrichment_candidates, update_edition_identity_candidate_classifications,
};
pub(crate) use remote_reducer::apply_remote_change_in_tx;

fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn nonnegative_u64(value: i64, column: usize, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} must not be negative")))
}

fn bounded_u8(value: i64, column: usize, name: &str) -> rusqlite::Result<u8> {
    u8::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} is outside its storage range")))
}

fn function_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn function_timestamp(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<UnixMillis> {
    let value = context.get::<i64>(index)?;
    u64::try_from(value).map_err(|_| function_error("negative mutation timestamp"))
}

fn function_book_format(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<BookFormat> {
    let raw = context.get::<String>(index)?;
    BookFormat::from_extension(&raw).ok_or_else(|| function_error(format!("book format '{raw}' is not a supported book format")))
}

fn function_content_hash(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<SyncContentHash> {
    Ok(SyncContentHash::new(&context.get::<String>(index)?))
}

fn function_contributors(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<Vec<Contributor>> {
    let bytes = context.get::<Vec<u8>>(index)?;
    sync_common::wire::decode(&bytes, 64 * 1024).map_err(|error| function_error(error.to_string()))
}

/// Reads the current `book_contributor` rows for one book (ordered by their
/// shared flat position sequence) and wire-encodes them. Triggers call this
/// instead of decoding a passed-in blob, since the canonical `book.authors`
/// column no longer exists — contributor state now only lives relationally.
fn contributors_for_book(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
    let book_row_id = context.get::<i64>(0)?;
    // SAFETY: `contributors_for_book` is only invoked from within triggers
    // firing on the same connection that owns this function registration;
    // sqlite guarantees the context's db handle is that connection while the
    // triggering statement is executing.
    let connection = unsafe { context.get_connection()? };
    let mut statement = connection.prepare_cached("SELECT author_identity_id, name, sort_name, role FROM book_contributor WHERE book_row_id = ?1 ORDER BY position")?;
    let mut rows = statement.query([book_row_id])?;
    let mut contributors = Vec::new();
    while let Some(row) = rows.next()? {
        let author_identity_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = library_model::MarcRelatorCode(role_array);
        let stable_id: String = connection.query_row("SELECT stable_id FROM author_identity WHERE id = ?1", [author_identity_id], |identity_row| identity_row.get(0))?;
        let contributor_id = AgentId::parse_str(&stable_id).map_err(|error| function_error(error.to_string()))?;
        contributors.push(Contributor::with_id(contributor_id, name, sort_name, role).map_err(|error| function_error(error.to_string()))?);
    }
    sync_common::wire::encode(&contributors).map_err(|error| function_error(error.to_string()))
}

fn function_publication(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<PublicationMetadata> {
    let bytes = context.get::<Vec<u8>>(index)?;
    if bytes.is_empty() {
        return Ok(PublicationMetadata::default());
    }
    sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| function_error(error.to_string()))
}

fn function_annotation_style(value: &str) -> rusqlite::Result<AnnotationStyle> {
    match value {
        "highlight" => Ok(AnnotationStyle::Highlight),
        "underline" => Ok(AnnotationStyle::Underline),
        "squiggly" => Ok(AnnotationStyle::Squiggly),
        "strikethrough" => Ok(AnnotationStyle::Strikethrough),
        _ => Err(function_error("invalid annotation style")),
    }
}

fn encode_trigger_mutation(context: &FunctionContext<'_>) -> rusqlite::Result<Vec<u8>> {
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
        "book_lifecycle" if context.len() == 10 => {
            let content_hash = function_content_hash(context, 1)?;
            let value = match context.get::<i64>(2)? {
                0 => BookLifecycleState::Present {
                    metadata: BookMetadataState { title: context.get(3)?, contributors: function_contributors(context, 4)?, description: context.get(5)?, publication: function_publication(context, 6)? },
                    added_at: function_timestamp(context, 7)?,
                    // A book is only ever added by the scanner, which refuses to
                    // index a file whose extension it cannot classify, so a live
                    // row always carries a format. There is deliberately no
                    // fallback: an unnamed format would silently become a wrong
                    // filename on every device that downloads the book.
                    format: function_book_format(context, 8)?,
                },
                1 => BookLifecycleState::Deleted { origin_folder_id: context.get(9)? },
                2 => BookLifecycleState::Purged,
                _ => return Err(function_error("invalid book lifecycle state")),
            };
            MutationBody::BookLifecycle { content_hash, value }
        }
        "metadata" if context.len() == 6 => MutationBody::Metadata {
            content_hash: function_content_hash(context, 1)?,
            value: BookMetadataState { title: context.get(2)?, contributors: function_contributors(context, 3)?, description: context.get(4)?, publication: function_publication(context, 5)? },
        },
        "placement" if context.len() == 5 => {
            let dir_id = function_uuid(context, 1)?;
            let content_hash = function_content_hash(context, 2)?;
            let present = context.get(3)?;
            MutationBody::Placement { dir_id, content_hash, present, origin_folder_id: context.get(4)? }
        }
        "reading_position" if context.len() == 4 => MutationBody::ReadingPosition {
            content_hash: function_content_hash(context, 1)?,
            value: ReadingPositionState { location: library_model::ReadingPosition::parse(&context.get::<String>(2)?).map_err(|error| function_error(error.to_string()))?, progress: context.get(3)? },
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
        "bookmark" if context.len() == 8 => MutationBody::Bookmark {
            bookmark_id: context.get(1)?,
            value: BookmarkState {
                content_hash: function_content_hash(context, 2)?,
                cfi: context.get(3)?,
                label: context.get(4)?,
                created_at: function_timestamp(context, 5)?,
                modified_at: function_timestamp(context, 6)?,
                deleted: context.get(7)?,
            },
        },
        "book_link" if context.len() == 9 => MutationBody::BookLink {
            link_id: context.get(1)?,
            value: BookLinkState {
                source_content_hash: function_content_hash(context, 2)?,
                target_content_hash: function_content_hash(context, 3)?,
                relation: context.get(4)?,
                note: context.get(5)?,
                created_at: function_timestamp(context, 6)?,
                modified_at: function_timestamp(context, 7)?,
                deleted: context.get(8)?,
            },
        },
        "catalog_publication" if context.len() == 5 => MutationBody::CatalogPublication {
            source_id: CatalogSourceId::parse(&context.get::<String>(1)?).map_err(function_error)?,
            publication: CatalogPublicationIdentity { edition_id: context.get(2)?, work_id: context.get(3)? },
            content_hash: function_content_hash(context, 4)?,
        },
        _ => return Err(function_error(format!("invalid {kind} trigger mutation arguments"))),
    };
    sync_common::wire::encode(&body).map_err(|error| function_error(error.to_string()))
}

fn register_sync_functions(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function("bokheim_mutation", -1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, encode_trigger_mutation)?;
    conn.create_scalar_function("catalog_publication_state_subkey", 2, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| {
        let edition_id = context.get::<String>(0)?;
        let work_id = context.get::<Option<String>>(1)?;
        Ok(catalog_publication_state_subkey(&edition_id, work_id.as_deref()))
    })?;
    conn.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_model::normalize_search_text(&context.get::<String>(0)?)))?;
    conn.create_scalar_function("portable_name_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_model::portable_name_key(&context.get::<String>(0)?)))?;
    // Not SQLITE_DETERMINISTIC: this function re-enters the connection to
    // query live book_contributor state, so its result depends on database
    // contents, not solely its arguments.
    conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, |context| contributors_for_book(context))
}

pub(crate) fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(Duration::from_secs(10))?;
    register_sync_functions(conn)
}

fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

fn function_uuid(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let value = context.get::<String>(index)?;
    Uuid::parse_str(&value).map_err(|error| function_error(error.to_string()))
}

fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

#[derive(Clone, Debug)]
struct StoredVersion(VersionKey);

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
        Ok(Some(Self(VersionKey::new(changed_at, conflict_rank, actor, actor_seq, mutation_id))))
    }

    fn into_key(self) -> VersionKey {
        self.0
    }

    fn decode_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<Option<VersionKey>> {
        Self::decode(row.get(offset)?, row.get(offset + 1)?, row.get(offset + 2)?, row.get(offset + 3)?, row.get(offset + 4)?, [offset, offset + 1, offset + 2, offset + 3, offset + 4]).map(|version| version.map(Self::into_key))
    }
}

fn unix_millis() -> Result<i64, rusqlite::Error> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_millis();
    i64::try_from(millis).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn unix_secs() -> Result<i64, rusqlite::Error> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_secs();
    i64::try_from(seconds).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub fn init_db(conn: &Connection) -> Result<ReplicaId, rusqlite::Error> {
    configure_connection(conn)?;

    conn.create_author_identity_table()?;
    conn.create_author_identity_stable_id_index()?;
    conn.create_author_identity_normalized_name_index()?;
    conn.create_author_external_identifier_table()?;
    conn.create_book_table()?;
    conn.create_publication_metadata_projection_tables()?;
    conn.create_subject_taxonomy_tables()?;
    conn.create_external_metadata_attempt_table()?;
    external_metadata::migrate_legacy_edition_identity_candidates(conn)?;
    conn.create_external_entity_tables()?;
    seed_subject_systems(conn)?;
    conn.create_catalog_publication_book_table()?;
    conn.drop_catalog_publication_work_index()?;
    conn.create_dir_table()?;
    conn.insert_root()?;
    conn.create_live_dir_path_index()?;
    conn.create_book_dir_table()?;
    conn.create_book_dir_local_hash_index()?;
    conn.create_book_dir_book_index()?;
    conn.create_live_book_path_index()?;
    conn.create_sync_outbox_table()?;
    conn.create_sync_state_version_table()?;
    conn.create_annotation_table()?;
    conn.create_annotation_book_index()?;
    conn.create_bookmark_table()?;
    conn.create_bookmark_book_index()?;
    conn.create_book_link_table()?;
    conn.create_book_link_source_index()?;
    conn.create_book_link_target_index()?;
    conn.create_sync_metadata_table()?;
    conn.create_remote_asset_table()?;
    conn.create_blob_local_ref_table()?;
    conn.create_download_requests_table()?;
    conn.create_audiobook_metadata_table()?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS transfer_history(
            id TEXT PRIMARY KEY NOT NULL,
            payload TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_transfer_history_updated_at
            ON transfer_history(updated_at DESC);",
    )?;
    conn.create_views()?;
    conn.execute_batch(include_str!("sql/triggers.sql"))?;
    conn.set_sync_metadata("change_origin", "local")?;
    let restored = external_metadata::migrate_automatic_alternative_edition_matches(conn)?;
    if restored > 0 {
        log::info!("restored {restored} automatically accepted alternative-edition matches to unresolved review state");
    }

    conn.pragma_update(None, "user_version", LIBRARY_SCHEMA_VERSION)?;

    get_or_create_replica_id(conn)
}

fn seed_subject_systems(conn: &Connection) -> Result<(), rusqlite::Error> {
    let system = bisac_subject_system();
    let transaction = conn.unchecked_transaction()?;
    let stored_version = transaction.query_row("SELECT version FROM subject_system WHERE id=?1", [system.id()], |row| row.get::<_, String>(0)).ok();
    let taxonomy_changed = stored_version.as_deref() != Some(system.version());
    if taxonomy_changed {
        transaction.execute("DELETE FROM subject_system WHERE id=?1", [system.id()])?;
        transaction.execute("INSERT INTO subject_system(id,version,matcher_version) VALUES(?1,?2,?3)", rusqlite::params![system.id(), system.version(), BISAC_MATCHER_VERSION])?;
    } else {
        transaction.execute("UPDATE subject_system SET matcher_version=?2 WHERE id=?1", rusqlite::params![system.id(), BISAC_MATCHER_VERSION])?;
    }
    if taxonomy_changed {
        let mut insert = transaction.prepare_cached("INSERT INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(?1,?2,?3,?4,?5,?6)")?;
        for node in system.nodes() {
            insert.execute(rusqlite::params![system.id(), node.path(), node.parent_path().unwrap_or(""), node.name(), node.code(), i32::from(node.code().is_some())])?;
        }
    }
    transaction.commit()?;
    seed_ddc_subject_assignments(conn)?;
    seed_lcc_subject_assignments(conn)?;
    seed_unified_subject_assignments(conn)
}

fn seed_ddc_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    let stored = conn.query_row("SELECT version,matcher_version FROM subject_system WHERE id=?1", [DDC_SYSTEM_ID], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).ok();
    if stored.as_ref().is_some_and(|(version, matcher)| version == DDC_VERSION && *matcher == DDC_MATCHER_VERSION) {
        return Ok(());
    }

    let declarations = {
        let mut statement = conn.prepare(
            "SELECT book_row_id,position,name,code FROM book_subject
             WHERE LOWER(TRIM(authority))=?1 ORDER BY book_row_id,position",
        )?;
        let rows = statement.query_map([DDC_SYSTEM_ID], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM subject_system WHERE id=?1", [DDC_SYSTEM_ID])?;
    transaction.execute("INSERT INTO subject_system(id,version,matcher_version) VALUES(?1,?2,?3)", rusqlite::params![DDC_SYSTEM_ID, DDC_VERSION, DDC_MATCHER_VERSION])?;
    for (book_row_id, position, name, code) in declarations {
        let Some(subject_path) = ddc_subject_path(code.as_deref().unwrap_or(&name)) else { continue };
        let mut path = String::new();
        let mut parent_path = String::new();
        for component in subject_path.split(" / ") {
            if !path.is_empty() {
                path.push_str(" / ");
            }
            path.push_str(component);
            transaction.execute("INSERT OR IGNORE INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(?1,?2,?3,?4,NULL,1)", rusqlite::params![DDC_SYSTEM_ID, path, parent_path, component])?;
            parent_path.clone_from(&path);
        }
        transaction.execute("INSERT OR IGNORE INTO book_subject_assignment(book_row_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4)", rusqlite::params![book_row_id, DDC_SYSTEM_ID, subject_path, DDC_MATCHER_VERSION])?;
        transaction.execute("INSERT OR IGNORE INTO book_subject_assignment_evidence(book_row_id,system_id,subject_path,subject_position) VALUES(?1,?2,?3,?4)", rusqlite::params![book_row_id, DDC_SYSTEM_ID, subject_path, position])?;
    }
    transaction.commit()
}

fn seed_lcc_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    let stored = conn.query_row("SELECT version,matcher_version FROM subject_system WHERE id=?1", [LCC_SYSTEM_ID], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).ok();
    if stored.as_ref().is_some_and(|(version, matcher)| version == LCC_VERSION && *matcher == LCC_MATCHER_VERSION) {
        return Ok(());
    }

    let declarations = {
        let mut statement = conn.prepare(
            "SELECT book_row_id,position,name,code FROM book_subject
             WHERE LOWER(TRIM(authority))=?1 ORDER BY book_row_id,position",
        )?;
        let rows = statement.query_map([LCC_SYSTEM_ID], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM subject_system WHERE id=?1", [LCC_SYSTEM_ID])?;
    transaction.execute("INSERT INTO subject_system(id,version,matcher_version) VALUES(?1,?2,?3)", rusqlite::params![LCC_SYSTEM_ID, LCC_VERSION, LCC_MATCHER_VERSION])?;
    for (book_row_id, position, name, code) in declarations {
        let notation = code.as_deref().unwrap_or(&name);
        let subject_path = lcc_subject_path(notation).unwrap_or_else(|| format!("LCC / Unclassified / {}", notation.split_whitespace().collect::<Vec<_>>().join(" ")));
        let mut path = String::new();
        let mut parent_path = String::new();
        for component in subject_path.split(" / ") {
            if !path.is_empty() {
                path.push_str(" / ");
            }
            path.push_str(component);
            transaction.execute("INSERT OR IGNORE INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(?1,?2,?3,?4,NULL,1)", rusqlite::params![LCC_SYSTEM_ID, path, parent_path, component])?;
            parent_path.clone_from(&path);
        }
        transaction.execute("INSERT OR IGNORE INTO book_subject_assignment(book_row_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4)", rusqlite::params![book_row_id, LCC_SYSTEM_ID, subject_path, LCC_MATCHER_VERSION])?;
        transaction.execute("INSERT OR IGNORE INTO book_subject_assignment_evidence(book_row_id,system_id,subject_path,subject_position) VALUES(?1,?2,?3,?4)", rusqlite::params![book_row_id, LCC_SYSTEM_ID, subject_path, position])?;
    }
    transaction.commit()
}

fn seed_unified_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    let release_id = library_model::unified_taxonomy_release_id();
    let stored = conn.query_row("SELECT version,matcher_version FROM subject_system WHERE id=?1", [UNIFIED_SYSTEM_ID], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).ok();
    if stored.as_ref().is_some_and(|(version, matcher)| version == &release_id && *matcher == UNIFIED_MATCHER_VERSION) {
        return Ok(());
    }

    let declarations = {
        let mut statement = conn.prepare("SELECT book_row_id,position,name,source,authority,code FROM book_subject ORDER BY book_row_id,position")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut books = std::collections::BTreeMap::<i64, Vec<(i64, PublicationSubject)>>::new();
    for (book_row_id, position, name, source, authority, code) in declarations {
        let subject = PublicationSubject::new(None, &name, &source, authority, code).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        books.entry(book_row_id).or_default().push((position, subject));
    }

    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM subject_system WHERE id=?1", [UNIFIED_SYSTEM_ID])?;
    transaction.execute("DELETE FROM subject_code_mapping", [])?;
    transaction.execute("DELETE FROM book_subject_code", [])?;
    transaction.execute("DELETE FROM book_unified_concept", [])?;
    transaction.execute("DELETE FROM source_concept_mapping", [])?;
    transaction.execute("DELETE FROM source_subject", [])?;
    transaction.execute("DELETE FROM unified_concept_route", [])?;
    transaction.execute("DELETE FROM unified_concept_edge", [])?;
    transaction.execute("DELETE FROM unified_concept", [])?;
    transaction.execute("DELETE FROM unified_taxonomy_release", [])?;
    transaction.execute("INSERT INTO unified_taxonomy_release(singleton,version,mapper_version) VALUES(1,?1,?2)", rusqlite::params![release_id.as_str(), UNIFIED_MATCHER_VERSION])?;
    let routes = library_model::unified_taxonomy_routes();
    for route in &routes {
        transaction
            .execute("INSERT OR IGNORE INTO unified_concept(concept_id,preferred_label,description,status,introduced_version) VALUES(?1,?2,NULL,'active',?3)", rusqlite::params![route.concept_id(), route.label(), release_id.as_str()])?;
    }
    for route in &routes {
        transaction.execute("INSERT INTO unified_concept_route(path,concept_id,parent_path,label) VALUES(?1,?2,?3,?4)", rusqlite::params![route.path(), route.concept_id(), route.parent_path(), route.label()])?;
        if !route.parent_path().is_empty() {
            transaction.execute(
                "INSERT OR IGNORE INTO unified_concept_edge(parent_concept_id,child_concept_id,relationship,sort_order) VALUES(?1,?2,'broader',0)",
                rusqlite::params![library_model::unified_concept_id(route.parent_path()), route.concept_id()],
            )?;
        }
    }
    transaction.execute("INSERT INTO subject_system(id,version,matcher_version) VALUES(?1,?2,?3)", rusqlite::params![UNIFIED_SYSTEM_ID, release_id.as_str(), UNIFIED_MATCHER_VERSION])?;
    for (book_row_id, declarations) in books {
        let subjects = declarations.iter().map(|(_, subject)| subject.clone()).collect::<Vec<_>>();
        let positions = declarations.iter().map(|(position, _)| *position).collect::<Vec<_>>();
        let codes = library_model::project_subject_codes(&subjects);
        let assignments = library_model::project_unified_subjects(&subjects);
        for code in &codes {
            for local_position in code.evidence_positions() {
                transaction.execute("INSERT OR IGNORE INTO book_subject_code(book_row_id,subject_position,system_id,code) VALUES(?1,?2,?3,?4)", rusqlite::params![book_row_id, positions[*local_position], code.system_id(), code.code()])?;
            }
        }
        for assignment in &assignments {
            ensure_seeded_subject_nodes(&transaction, assignment.subject_path())?;
            transaction.execute(
                "INSERT OR IGNORE INTO book_subject_assignment(book_row_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4)",
                rusqlite::params![book_row_id, UNIFIED_SYSTEM_ID, assignment.subject_path(), UNIFIED_MATCHER_VERSION],
            )?;
            for local_position in assignment.evidence_positions() {
                transaction.execute(
                    "INSERT OR IGNORE INTO book_subject_assignment_evidence(book_row_id,system_id,subject_path,subject_position) VALUES(?1,?2,?3,?4)",
                    rusqlite::params![book_row_id, UNIFIED_SYSTEM_ID, assignment.subject_path(), positions[*local_position]],
                )?;
            }
        }
        for code in &codes {
            for unified_path in library_model::unified_subject_paths(code.system_id(), code.code()) {
                let source_version = source_subject_version(code.system_id());
                let concept_id = library_model::unified_concept_id(&unified_path);
                transaction.execute("INSERT OR IGNORE INTO source_subject(system_id,source_version,code,official_label) VALUES(?1,?2,?3,NULL)", rusqlite::params![code.system_id(), source_version, code.code()])?;
                transaction.execute(
                    "INSERT OR REPLACE INTO source_concept_mapping(source_system_id,source_version,source_code,concept_id,mapping_type,mapper_version) VALUES(?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![code.system_id(), source_version, code.code(), concept_id, source_mapping_type(code.system_id()), UNIFIED_MATCHER_VERSION],
                )?;
                transaction.execute("INSERT OR IGNORE INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES(?1,?2,?3)", rusqlite::params![book_row_id, concept_id, UNIFIED_MATCHER_VERSION])?;
                for local_position in code.evidence_positions() {
                    transaction.execute(
                        "INSERT OR IGNORE INTO book_unified_concept_evidence(book_row_id,concept_id,subject_position,source_system_id,source_code) VALUES(?1,?2,?3,?4,?5)",
                        rusqlite::params![book_row_id, concept_id, positions[*local_position], code.system_id(), code.code()],
                    )?;
                }
                transaction.execute(
                    "INSERT OR REPLACE INTO subject_code_mapping(source_system_id,source_code,unified_path,mapper_version) VALUES(?1,?2,?3,?4)",
                    rusqlite::params![code.system_id(), code.code(), unified_path, UNIFIED_MATCHER_VERSION],
                )?;
            }
        }
    }
    transaction.commit()
}

pub(crate) fn reload_unified_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    seed_unified_subject_assignments(conn)
}

/// Returns every supported external identifier recorded for a book. Exact
/// local coverage is evaluated against the device taxonomy by BackendContext.
pub(crate) fn recorded_taxonomy_codes(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<TaxonomyCode>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT subject.system_id,subject.code
         FROM book_subject_code subject
         JOIN book ON book.row_id=subject.book_row_id
         WHERE book.content_hash=?1
         ORDER BY subject.system_id,subject.code",
    )?;
    let rows = statement.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut codes = Vec::new();
    for row in rows {
        let (system_id, code) = row?;
        let system = match system_id.as_str() {
            library_model::BISAC_SYSTEM_ID => TaxonomySystem::Bisac,
            library_model::DDC_SYSTEM_ID => TaxonomySystem::Ddc,
            library_model::LCC_SYSTEM_ID => TaxonomySystem::Lcc,
            _ => continue,
        };
        codes.push(TaxonomyCode { system, code });
    }
    Ok(codes)
}

pub(crate) fn recorded_taxonomy_codes_for_library(conn: &Connection) -> Result<Vec<TaxonomyCode>, rusqlite::Error> {
    let mut statement = conn.prepare("SELECT DISTINCT system_id,code FROM book_subject_code ORDER BY system_id,code")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut codes = Vec::new();
    for row in rows {
        let (system_id, code) = row?;
        let system = match system_id.as_str() {
            library_model::BISAC_SYSTEM_ID => TaxonomySystem::Bisac,
            library_model::DDC_SYSTEM_ID => TaxonomySystem::Ddc,
            library_model::LCC_SYSTEM_ID => TaxonomySystem::Lcc,
            _ => continue,
        };
        codes.push(TaxonomyCode { system, code });
    }
    Ok(codes)
}

fn source_subject_version(system_id: &str) -> &'static str {
    match system_id {
        library_model::BISAC_SYSTEM_ID => library_model::BISAC_VERSION,
        library_model::DDC_SYSTEM_ID => library_model::DDC_VERSION,
        library_model::LCC_SYSTEM_ID => library_model::LCC_VERSION,
        _ => "external",
    }
}

fn source_mapping_type(system_id: &str) -> &'static str {
    match system_id {
        library_model::BISAC_SYSTEM_ID => "exact",
        library_model::DDC_SYSTEM_ID | library_model::LCC_SYSTEM_ID => "broader",
        _ => "related",
    }
}

fn ensure_seeded_subject_nodes(transaction: &Transaction<'_>, subject_path: &str) -> Result<(), rusqlite::Error> {
    let mut path = String::new();
    let mut parent_path = String::new();
    for component in subject_path.split(" / ") {
        if !path.is_empty() {
            path.push_str(" / ");
        }
        path.push_str(component);
        transaction.execute("INSERT OR IGNORE INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(?1,?2,?3,?4,NULL,1)", rusqlite::params![UNIFIED_SYSTEM_ID, path, parent_path, component])?;
        parent_path.clone_from(&path);
    }
    Ok(())
}

pub fn get_or_create_replica_id(db: &Connection) -> Result<ReplicaId, rusqlite::Error> {
    let mut replica_id: Option<String> = None;
    db.get_sync_metadata_value("replica_id", |row| {
        replica_id = Some(row.get(0)?);
        Ok(())
    })?;

    match replica_id {
        Some(id) => parse_uuid_column(id, 0),
        None => {
            let new_id = ReplicaId::new_v4();
            db.set_sync_metadata("replica_id", &new_id.to_string())?;
            Ok(new_id)
        }
    }
}

/// Read models used by library browsing pages.
pub(crate) mod browse;
pub(crate) use browse::{get_book_ancestor_dir_ids, get_descendant_content_hashes, get_folder_download_counts};

mod catalog;
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use browse::{KnownDirectory, KnownDirectoryState, get_known_directories_for_scan, get_known_directory_scan_state, is_book_file_tombstoned};
pub(crate) use catalog::{catalog_recommendation_request, catalog_subject_recommendation_request, owned_catalog_publications, record_catalog_publication};

/// Local and remote asset state, paths, and durable download intent.
pub(crate) mod assets;
pub(crate) use assets::{
    BookPath, add_asset_request, book_placement_expects_hash, complete_asset_request, forget_remote_asset, get_asset_requests, get_blob_local_path, get_book_assets, get_book_dir_entry_by_path, get_book_file_path, get_book_format,
    get_book_hashes, get_book_paths_by_content_hash, get_live_dir_paths_tx, get_locally_present_blob_hashes, get_remote_asset_hashes, is_blob_local, is_book_download_requested, is_book_downloaded, local_file_fingerprint,
    materialize_dir_parent_with_markers, materialize_dir_with_markers, materialize_download_path, record_remote_asset, resolve_removed_book_dir, stamp_book_dir_local_hash_by_rel_path, upsert_blob_local_ref,
};

/// Durable reader progress for EPUB, PDF, and audiobook formats.
pub(crate) mod reading;
#[cfg(test)]
pub(crate) use reading::{get_audiobook_reading_position, get_book_reading_position, get_pdf_reading_page};
pub(crate) use reading::{update_pdf_reading_page, update_reading_position};

/// Synchronization outbox, scan generations, tombstones, and filesystem work.
pub(crate) mod sync_state;
pub(crate) use sync_state::{
    LibrarySyncDetails, RemovedBookDir, acknowledge_mutations, enqueue_missing_state_cells, get_library_sync_details, get_pending_mutations, get_pull_cursor, get_sync_metadata, next_scan_id, rebind_sync_account, set_pull_cursor,
    set_sync_metadata, state_inventory,
};
#[cfg(test)]
pub(crate) use sync_state::{OutboxReadError, OutboxRow, decode_outbox_row};
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use sync_state::{count_pending_vanish_candidates, mark_vanished_book_dirs_unavailable};

/// Typed book-card read models.
pub(crate) mod books;
#[cfg(test)]
pub(crate) use books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};

/// Metadata registers, relational projections, and author identities.
mod authors;
pub(crate) mod metadata;
pub(crate) use metadata::insert_scanned_book_metadata;
#[cfg(test)]
pub(crate) use metadata::{
    attach_external_author_id, get_book_author_identities, insert_scanned_book, insert_scanned_book_contributors, resolve_book_author_identity, split_book_author, update_book_metadata, update_book_metadata_contributors,
    update_book_metadata_contributors_preserving_ids,
};

/// Directory and physical publication placement persistence.
pub(crate) mod placements;
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use placements::{book_has_live_placement, get_tombstoned_book_file_keys, mark_known_book_dirs_seen};
pub(crate) use placements::{create_local_directory, directory_relative_path, insert_local_book_dir, set_book_downloaded};

/// Metadata derived specifically from audiobook files.
mod audiobooks;
pub(crate) use audiobooks::upsert_audiobook_metadata;

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use placements::{get_book_dirs_by_local_fingerprint, insert_book, upsert_dir};

#[cfg(test)]
mod tests;
