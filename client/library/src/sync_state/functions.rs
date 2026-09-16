use catalog_model::CatalogPublicationIdentity;
use catalog_model::CatalogSourceId;
use library_model::AgentId;
use library_model::AnnotationAnchor;
use library_model::AnnotationState;
use library_model::AnnotationStyle;
use library_model::BookFormat;
use library_model::BookLifecycleState;
use library_model::Contributor;
use library_model::DirectoryLifecycleState;
use library_model::PdfAnnotationAnchor;
use library_model::PdfAnnotationRect;
use library_model::PublicationMetadata;
use library_model::ReadingPositionState;
use library_model::SyncBookMetadata;
use library_replica::MutationBody;
use library_replica::catalog_publication_state_subkey;
use rusqlite::Connection;
use rusqlite::functions::Context as FunctionContext;
use rusqlite::functions::FunctionFlags;
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
    let mut statement = connection.prepare_cached(include_str!("sql/contributors_for_book_select.sql"))?;
    let mut rows = statement.query([book_row_id])?;
    let mut contributors = Vec::new();
    while let Some(row) = rows.next()? {
        let author_identity_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let sort_name: Option<String> = row.get(2)?;
        let role_bytes: Vec<u8> = row.get(3)?;
        let role_array: [u8; 3] = role_bytes.as_slice().try_into().map_err(|_| function_error("stored contributor role must be exactly 3 bytes"))?;
        let role = library_model::MarcRelatorCode(role_array);
        let stable_id: String = connection.query_row(include_str!("sql/contributors_for_book_select_2.sql"), [author_identity_id], |identity_row| identity_row.get(0))?;
        let contributor_id = AgentId::parse_str(&stable_id).map_err(|error| function_error(error.to_string()))?;
        contributors.push(Contributor::with_id(contributor_id, name, sort_name, role).map_err(|error| function_error(error.to_string()))?);
    }
    sync_common::wire::encode(&contributors).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn function_publication(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<PublicationMetadata> {
    let bytes = context.get::<Vec<u8>>(index)?;
    if bytes.is_empty() {
        return Ok(PublicationMetadata::default());
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
                    metadata: SyncBookMetadata { title: context.get(3)?, contributors: function_contributors(context, 4)?, publication: function_publication(context, 5 + offset)? },
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
            value: SyncBookMetadata { title: context.get(2)?, contributors: function_contributors(context, 3)?, publication: function_publication(context, context.len() - 1)? },
        },
        "pdf_reader_metadata" if context.len() == 3 => {
            let bytes: Vec<u8> = context.get(2)?;
            let value = serde_json::from_slice(&bytes).map_err(|_| function_error("invalid PDF reader metadata"))?;
            MutationBody::PdfReaderMetadata { content_hash: function_content_hash(context, 1)?, value }
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
        "catalog_publication" if context.len() == 5 => MutationBody::CatalogPublication {
            source_id: CatalogSourceId::parse(&context.get::<String>(1)?).map_err(function_error)?,
            publication: CatalogPublicationIdentity { edition_id: context.get(2)?, work_id: context.get(3)? },
            content_hash: function_content_hash(context, 4)?,
        },
        _ => return Err(function_error(format!("invalid {kind} trigger mutation arguments"))),
    };
    sync_common::wire::encode(&body).map_err(|error| function_error(error.to_string()))
}

pub(crate) fn register_sync_functions(conn: &Connection) -> rusqlite::Result<()> {
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

pub(crate) fn function_uuid(context: &FunctionContext<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let value = context.get::<String>(index)?;
    Uuid::parse_str(&value).map_err(|error| function_error(error.to_string()))
}
