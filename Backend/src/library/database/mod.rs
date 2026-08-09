// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]

use crate::library::Dir;
use crate::sync::transfer::TransferOrigin;
use crate::{BlobKind, BookId, BookPlacement, ContentHash, RelativeBookPath};
use crate::{EpubCfi, ReadProgress};
use crate::{LibraryRoot, RelativeDirPath};
pub(crate) use bokheim_app_contract::library::BookCardRow;
use bokheim_domain::AuthorName;
#[cfg(test)]
use bokheim_domain::{AuthorAuthority, ExternalAuthorId};
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::functions::{Context as FunctionContext, FunctionFlags};
use rusqlite::types::Type;
use rusqlite::OptionalExtension;
use rusqlite::{Connection, Transaction};
#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use sync_common::MarcRelatorCode;
use sync_common::{
    AuthorId, BookId as SyncBookId, ClientId, Contributor, DirId, FileName, MutationId, PublicationIdentifierScheme, PublicationMetadata, PublisherCredit, ReplicaSeq, ServerMutation, UnixMillis,
};
use bokheim_domain::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookLifecycleState, BookLinkState, BookMetadataState, BookmarkState, MutationBody, PdfAnnotationAnchor, PdfAnnotationRect, ReadingPositionState, StateMutation, VersionKey};
use bokheim_domain::BookFormat;
use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;

include_sql!("src/library/database/sql/create_library.sql");
include_sql!("src/library/database/sql/assets.sql");
include_sql!("src/library/database/sql/audiobooks.sql");
include_sql!("src/library/database/sql/authors.sql");
include_sql!("src/library/database/sql/book_access.sql");
include_sql!("src/library/database/sql/books.sql");
include_sql!("src/library/database/sql/metadata.sql");
include_sql!("src/library/database/sql/browsing.sql");
include_sql!("src/library/database/sql/placements.sql");
include_sql!("src/library/database/sql/remote_reducer.sql");
include_sql!("src/library/database/sql/scanner.sql");
include_sql!("src/library/database/sql/sync.sql");
include_sql!("src/library/database/sql/views.sql");

// Bumped 13 -> 14 to merge `book_author`/`book_contributor` into one
// relational contributor register (see Contributor in SyncCommon) and drop
// the wire-encoded `book.authors` BLOB column. `init_db` reacts to an older
// stored `user_version` by dropping+recreating only the purely rescannable
// projection tables (`book_author`, `book_contributor`) and altering `book`
// to drop `authors` — never by dropping `book` itself, since `annotation`,
// `bookmark`, and `book_link` cascade off `book.id` and hold local-only user
// data that isn't rescannable from disk. This drop-and-rescan approach is a
// deliberate pre-1.0 choice, not a permanent migration strategy.
pub(crate) const LIBRARY_SCHEMA_VERSION: i64 = 14;

/// Targeted upgrade for a database created before the merged contributor
/// schema. A no-op on a fresh database (`stored_version == 0`) or one already
/// current.
fn migrate_legacy_contributor_schema(conn: &Connection) -> rusqlite::Result<()> {
    let stored_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if stored_version == 0 || stored_version >= LIBRARY_SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch("DROP TABLE IF EXISTS book_author; DROP TABLE IF EXISTS book_contributor; DROP VIEW IF EXISTS author_credit; DROP VIEW IF EXISTS book_cards;")?;
    let has_authors_column = conn.prepare("SELECT 1 FROM pragma_table_info('book') WHERE name = 'authors'")?.query_row([], |row| row.get::<_, i64>(0)).optional()?.is_some();
    if has_authors_column {
        conn.execute_batch("ALTER TABLE book DROP COLUMN authors;")?;
    }
    Ok(())
}

fn fallback_content_hash() -> ContentHash {
    ContentHash::parse(&"0".repeat(64)).unwrap()
}

pub(crate) fn directory_exists(conn: &Connection, directory_id: &str) -> rusqlite::Result<bool> {
    let mut exists = false;
    conn.live_directory_exists(directory_id, |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}

mod links;
mod marks;
mod remote_reducer;

use assets::materialize_dir_components_with_markers;
use books::read_book_card_row;

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
    UnixMillis::new(u64::try_from(value).map_err(|_| function_error("negative mutation timestamp"))?).map_err(|error| function_error(error.to_string()))
}

fn function_book_format(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<BookFormat> {
    let raw = context.get::<String>(index)?;
    BookFormat::from_extension(&raw).ok_or_else(|| function_error(format!("book format '{raw}' is not a supported book format")))
}

fn function_book_id(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<SyncBookId> {
    SyncBookId::parse(&context.get::<String>(index)?).map_err(|error| function_error(error.to_string()))
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
    let book_id = context.get::<String>(0)?;
    // SAFETY: `contributors_for_book` is only invoked from within triggers
    // firing on the same connection that owns this function registration;
    // sqlite guarantees the context's db handle is that connection while the
    // triggering statement is executing.
    let connection = unsafe { context.get_connection()? };
    let mut statement = connection.prepare_cached("SELECT author_identity_id, name, sort_name, role FROM book_contributor WHERE book_id = ?1 ORDER BY position")?;
    let mut rows = statement.query([&book_id])?;
    let mut contributors = Vec::new();
    while let Some(row) = rows.next()? {
        let author_identity_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = sync_common::MarcRelatorCode(role_array);
        let stable_id: String = connection.query_row("SELECT stable_id FROM author_identity WHERE id = ?1", [author_identity_id], |identity_row| identity_row.get(0))?;
        let contributor_id = AuthorId::parse(&stable_id).map_err(|error| function_error(error.to_string()))?;
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
            let dir_id = DirId::parse(&context.get::<String>(1)?).map_err(|error| function_error(error.to_string()))?;
            let name = FileName::parse(&context.get::<String>(2)?).map_err(|error| function_error(error.to_string()))?;
            MutationBody::DirectoryName { dir_id, name }
        }
        "directory_parent" if context.len() == 3 => {
            let dir_id = DirId::parse(&context.get::<String>(1)?).map_err(|error| function_error(error.to_string()))?;
            let parent_id = DirId::parse(&context.get::<String>(2)?).map_err(|error| function_error(error.to_string()))?;
            MutationBody::DirectoryParent { dir_id, parent_id }
        }
        "directory_lifecycle" if context.len() == 3 => {
            let dir_id = DirId::parse(&context.get::<String>(1)?).map_err(|error| function_error(error.to_string()))?;
            MutationBody::DirectoryLifecycle { dir_id, deleted: context.get(2)? }
        }
        "book_lifecycle" if context.len() == 9 || context.len() == 10 => {
            let book_id = function_book_id(context, 1)?;
            let value = if context.get::<bool>(2)? {
                BookLifecycleState::Deleted
            } else {
                BookLifecycleState::Present {
                    metadata: BookMetadataState { title: context.get(3)?, contributors: function_contributors(context, 4)?, description: context.get(5)?, publication: function_publication(context, 6)? },
                    added_at: function_timestamp(context, 7)?,
                    content_hash: context.get::<String>(8).map(|raw| ContentHash::parse(&raw).unwrap_or_else(|_| fallback_content_hash())).unwrap_or_else(|_| fallback_content_hash()),
                    // A book is only ever added by the scanner, which refuses to
                    // index a file whose extension it cannot classify, so a live
                    // row always carries a format. There is deliberately no
                    // fallback: an unnamed format would silently become a wrong
                    // filename on every device that downloads the book.
                    format: function_book_format(context, 9)?,
                }
            };
            MutationBody::BookLifecycle { book_id, value }
        }
        "metadata" if context.len() == 6 => MutationBody::Metadata {
            book_id: function_book_id(context, 1)?,
            value: BookMetadataState { title: context.get(2)?, contributors: function_contributors(context, 3)?, description: context.get(4)?, publication: function_publication(context, 5)? },
        },
        "placement" if context.len() == 4 => {
            let dir_id = DirId::parse(&context.get::<String>(1)?).map_err(|error| function_error(error.to_string()))?;
            let book_id = function_book_id(context, 2)?;
            let present = context.get(3)?;
            MutationBody::Placement { dir_id, book_id, present }
        }
        "reading_position" if context.len() == 4 => MutationBody::ReadingPosition {
            book_id: function_book_id(context, 1)?,
            value: ReadingPositionState { location: sync_common::ReadingPosition::parse(&context.get::<String>(2)?).map_err(|error| function_error(error.to_string()))?, progress: context.get(3)? },
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
                    book_id: function_book_id(context, 2)?,
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
            value: BookmarkState { book_id: function_book_id(context, 2)?, cfi: context.get(3)?, label: context.get(4)?, created_at: function_timestamp(context, 5)?, modified_at: function_timestamp(context, 6)?, deleted: context.get(7)? },
        },
        "book_link" if context.len() == 9 => MutationBody::BookLink {
            link_id: context.get(1)?,
            value: BookLinkState {
                source_book_id: function_book_id(context, 2)?,
                target_book_id: function_book_id(context, 3)?,
                relation: context.get(4)?,
                note: context.get(5)?,
                created_at: function_timestamp(context, 6)?,
                modified_at: function_timestamp(context, 7)?,
                deleted: context.get(8)?,
            },
        },
        _ => return Err(function_error(format!("invalid {kind} trigger mutation arguments"))),
    };
    sync_common::wire::encode(&body).map_err(|error| function_error(error.to_string()))
}

fn register_sync_functions(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function("bokheim_mutation", -1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, encode_trigger_mutation)?;
    // Not SQLITE_DETERMINISTIC: this function re-enters the connection to
    // query live book_contributor state, so its result depends on database
    // contents, not solely its arguments.
    conn.create_scalar_function("contributors_for_book", 1, FunctionFlags::SQLITE_UTF8, |context| contributors_for_book(context))
}

pub(crate) fn configure_sync_connection(conn: &Connection) -> rusqlite::Result<()> {
    register_sync_functions(conn)
}

fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

#[derive(Clone, Debug)]
struct StoredVersion(VersionKey);

impl StoredVersion {
    fn decode(changed_at: i64, conflict_rank: i64, actor: String, actor_seq: i64, mutation_id: String, columns: [usize; 5]) -> rusqlite::Result<Option<Self>> {
        if changed_at == 0 && actor.is_empty() && actor_seq == 0 && mutation_id.is_empty() {
            return Ok(None);
        }
        let actor = ClientId::parse(&actor).map_err(|error| invalid_text_column(columns[2], error.to_string()))?;
        let mutation_id = MutationId::parse(&mutation_id).map_err(|error| invalid_text_column(columns[4], error.to_string()))?;
        let changed_at = UnixMillis::new(nonnegative_u64(changed_at, columns[0], "change timestamp")?).map_err(|error| invalid_integer_column(columns[0], error.to_string()))?;
        let conflict_rank = bounded_u8(conflict_rank, columns[1], "conflict rank")?;
        let actor_seq = nonnegative_u64(actor_seq, columns[3], "actor sequence")?;
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

pub fn init_db(conn: &Connection) -> Result<ClientId, rusqlite::Error> {
    register_sync_functions(conn)?;
    migrate_legacy_contributor_schema(conn)?;

    conn.create_author_identity_table()?;
    conn.create_author_identity_stable_id_index()?;
    conn.create_author_identity_normalized_name_index()?;
    conn.create_author_external_identifier_table()?;
    conn.create_book_table()?;
    conn.create_publication_metadata_projection_tables()?;
    conn.drop_catalog_import_table()?;
    conn.create_dir_table()?;
    conn.insert_root()?;
    conn.create_live_dir_path_index()?;
    conn.create_book_dir_table()?;
    conn.create_book_dir_local_hash_index()?;
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
    conn.create_views()?;
    conn.execute_batch(include_str!("sql/triggers.sql"))?;
    conn.set_sync_metadata("change_origin", "local")?;

    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", "10000")?;
    conn.pragma_update(None, "user_version", LIBRARY_SCHEMA_VERSION)?;

    get_or_create_client_id(conn)
}

pub fn get_or_create_client_id(db: &Connection) -> Result<ClientId, rusqlite::Error> {
    let mut client_id: Option<String> = None;
    db.get_sync_metadata_value("client_id", |row| {
        client_id = Some(row.get(0)?);
        Ok(())
    })?;

    match client_id {
        Some(id) => ClientId::parse(&id).map_err(|error| invalid_text_column(0, error.to_string())),
        None => {
            let new_id = Uuid::new_v4().to_string();
            db.set_sync_metadata("client_id", &new_id)?;
            ClientId::parse(&new_id).map_err(|error| invalid_text_column(0, error.to_string()))
        }
    }
}

/// Read models used by library browsing pages.
pub(crate) mod browse;
pub(crate) use browse::{get_book_ancestor_dir_ids, get_descendant_book_ids, get_folder_download_counts};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use browse::{get_known_directory_scan_state, is_book_file_tombstoned, KnownDirectoryState};

/// Local and remote asset state, paths, and durable download intent.
pub(crate) mod assets;
pub(crate) use assets::{
    add_asset_request, book_placement_expects_hash, complete_asset_request, forget_remote_asset, get_asset_requests, get_blob_local_path, get_book_assets, get_book_dir_entry_by_path, get_book_file_path, get_book_hashes, materialize_download_path,
    get_book_paths_by_content_hash, get_live_dir_paths_tx, get_locally_present_blob_hashes, get_remote_asset_hashes, is_blob_local, is_book_download_requested, is_book_downloaded, local_file_fingerprint,
    materialize_dir_parent_with_markers, materialize_dir_with_markers, record_remote_asset, resolve_removed_book_dir, stamp_book_dir_local_hash_by_rel_path, upsert_blob_local_ref, BookPath,
};

/// Durable reader progress for EPUB, PDF, and audiobook formats.
pub(crate) mod reading;
#[cfg(test)]
pub(crate) use reading::{get_audiobook_reading_position, get_book_reading_position, get_pdf_reading_page};
pub(crate) use reading::{update_pdf_reading_page, update_reading_position};

/// Synchronization outbox, scan generations, tombstones, and filesystem work.
pub(crate) mod sync_state;
pub(crate) use sync_state::{acknowledge_mutations, get_pending_mutations, get_sync_metadata, next_scan_id, set_sync_metadata, RemovedBookDir};
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use sync_state::{count_pending_vanish_candidates, mark_vanished_book_dirs_unavailable};
#[cfg(test)]
pub(crate) use sync_state::{decode_outbox_row, OutboxReadError, OutboxRow};

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
pub(crate) use placements::{create_local_directory, insert_local_book_dir, set_book_downloaded};
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use placements::{book_has_live_placement, mark_known_book_dir_seen};

/// Metadata derived specifically from audiobook files.
mod audiobooks;
pub(crate) use audiobooks::upsert_audiobook_metadata;

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub(crate) use placements::{get_book_dirs_by_local_fingerprint, insert_book, upsert_dir};

#[cfg(test)]
mod tests;
