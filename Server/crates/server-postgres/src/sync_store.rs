use server_core::{LibraryUpsertOutcome, LiveAssets, UserThumbnail};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{
    AnnotationAnchor, AnnotationState, AnnotationStyle, AuthorCredit, AuthorId,
    BookId, BookLifecycleState, BookLinkState, BookMetadataState, BookmarkState,
    ClientId, ContentHash, DirId, DirectoryState, FileName, LibraryId,
    LibraryRevision, MutationBody, MutationId, MutationRejection,
    MutationRejectionReason, PdfAnnotationAnchor, PdfAnnotationRect,
    PlacementState, PublicationMetadata, ReadingPositionState, ReplicaSeq,
    ServerMutation, StateMutation, UnixMillis, MAX_PULL_CHANGES,
};

const LIST_LIBRARIES_SQL: &str = include_str!("sql/sync/list_libraries.sql");
const UPSERT_LIBRARY_SQL: &str = include_str!("sql/sync/upsert_library.sql");
const GET_LIBRARY_OWNER_SQL: &str = include_str!("sql/sync/get_library_owner.sql");
const ENSURE_LIBRARY_SQL: &str = include_str!("sql/sync/ensure_library.sql");
const OWNS_LIBRARY_SQL: &str = include_str!("sql/sync/owns_library.sql");
const LIVE_ASSETS_SQL: &str = include_str!("sql/sync/live_assets.sql");
const UPSERT_DIRECTORY_SQL: &str = include_str!("sql/sync/upsert_directory.sql");
const UPSERT_BOOK_PRESENT_SQL: &str = include_str!("sql/sync/upsert_book_present.sql");
const UPSERT_BOOK_DELETED_SQL: &str = include_str!("sql/sync/upsert_book_deleted.sql");
const UPSERT_PLACEMENT_PRESENT_SQL: &str = include_str!("sql/sync/upsert_placement_present.sql");
const UPSERT_PLACEMENT_VACANT_SQL: &str = include_str!("sql/sync/upsert_placement_vacant.sql");
const UPSERT_READING_POSITION_SQL: &str = include_str!("sql/sync/upsert_reading_position.sql");
const UPSERT_ANNOTATION_SQL: &str = include_str!("sql/sync/upsert_annotation.sql");
const UPSERT_BOOKMARK_SQL: &str = include_str!("sql/sync/upsert_bookmark.sql");
const UPSERT_BOOK_LINK_SQL: &str = include_str!("sql/sync/upsert_book_link.sql");
const UPSERT_LAST_OPENED_SQL: &str = include_str!("sql/sync/upsert_last_opened.sql");
const UPSERT_METADATA_SQL: &str = include_str!("sql/sync/upsert_metadata.sql");
const EXISTING_DEPENDENCIES_SQL: &str = include_str!("sql/sync/existing_dependencies.sql");
const UPSERT_DIRECTORY_BATCH_SQL: &str = include_str!("sql/sync/upsert_directory_batch.sql");
const UPSERT_BOOK_LIFECYCLE_BATCH_SQL: &str = include_str!("sql/sync/upsert_book_lifecycle_batch.sql");
const UPSERT_PLACEMENT_BATCH_SQL: &str = include_str!("sql/sync/upsert_placement_batch.sql");
const UPSERT_READING_POSITION_BATCH_SQL: &str = include_str!("sql/sync/upsert_reading_position_batch.sql");
const UPSERT_METADATA_BATCH_SQL: &str = include_str!("sql/sync/upsert_metadata_batch.sql");
const INSERT_AUTHORIZED_READING_POSITION_SQL: &str = include_str!("sql/sync/insert_authorized_reading_position.sql");
const EXCHANGE_AUTHORIZED_READING_POSITION_SQL: &str = include_str!("sql/sync/exchange_authorized_reading_position.sql");
const FETCH_PAGE_SQL: &str = include_str!("sql/sync/fetch_page.sql");
const GET_PULL_HEADS_SQL: &str = include_str!("sql/sync/get_pull_heads.sql");
const VALIDATE_OWNED_CURSOR_SQL: &str = include_str!("sql/sync/validate_owned_cursor.sql");
const FETCH_READING_POSITION_PAGE_SQL: &str = include_str!("sql/sync/fetch_reading_position_page.sql");

#[derive(serde::Deserialize)]
struct LegacyAuthorCredit {
    name: String,
    #[serde(default)]
    sort_name: Option<String>,
}

pub(super) async fn list_libraries(pool: &PgPool, user_id: &str) -> Result<Vec<LibrarySummary>, sqlx::Error> {
    let rows = sqlx::query(LIST_LIBRARIES_SQL).bind(user_id).fetch_all(pool).await?;
    rows.into_iter().map(|row| {
        let raw: String = row.try_get("id")?;
        Ok(LibrarySummary { library_id: LibraryId::parse(&raw).map_err(|error| sqlx::Error::Decode(Box::new(error)))?, library_name: row.try_get("name")? })
    }).collect()
}

pub(super) async fn upsert_library(pool: &PgPool, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, sqlx::Error> {
    let row = sqlx::query(UPSERT_LIBRARY_SQL).bind(request.library_id.as_str()).bind(user_id).bind(&request.library_name).fetch_optional(pool).await?;
    row.map(|row| {
        let raw: String = row.try_get("id")?;
        Ok(LibraryUpsertOutcome {
            library: LibrarySummary { library_id: LibraryId::parse(&raw).map_err(|error| sqlx::Error::Decode(Box::new(error)))?, library_name: row.try_get("name")? },
            changed: row.try_get("changed")?,
        })
    }).transpose()
}

pub(super) async fn ensure_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    if let Some(owner) = sqlx::query_scalar::<_, String>(GET_LIBRARY_OWNER_SQL).bind(library_id.as_str()).fetch_optional(pool).await? { return Ok(owner == user_id); }
    let owner = sqlx::query_scalar::<_, String>(ENSURE_LIBRARY_SQL)
        .bind(library_id.as_str()).bind(user_id).fetch_one(pool).await?;
    Ok(owner == user_id)
}

pub(super) async fn owns_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(OWNS_LIBRARY_SQL).bind(library_id.as_str()).bind(user_id).fetch_one(pool).await
}

pub(super) struct PlacementHashDelta { pub previous: Option<ContentHash>, pub current: Option<ContentHash> }
pub(super) struct InsertChangesResult { pub accepted: Vec<MutationId>, pub rejected: Vec<MutationRejection>, pub placement_deltas: Vec<PlacementHashDelta> }

pub(super) fn server_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}

pub(super) fn storage_rejection(change: &StateMutation, server_now_ms: u64) -> Option<MutationRejectionReason> {
    change.rejection_at(server_now_ms)
}

pub(super) fn supports_atomic_singleton(change: &StateMutation) -> bool { matches!(change.body, MutationBody::ReadingPosition { .. }) }

fn common<'a>(change: &StateMutation, client_id: &'a ClientId) -> (i64, &'a str, i64, i16, String) {
    (
        change.changed_at.storage_i64(),
        client_id.as_str(),
        change.replica_seq.get() as i64,
        change.body.conflict_rank() as i16,
        change.mutation_id.to_string(),
    )
}

fn encoded_authors(authors: &[AuthorCredit]) -> Result<Vec<u8>, sqlx::Error> {
    sync_common::wire::encode(&authors).map_err(|error| sqlx::Error::Protocol(error.to_string()))
}

fn encoded_publication(metadata: &PublicationMetadata) -> Result<Vec<u8>, sqlx::Error> {
    sync_common::wire::encode(metadata).map_err(|error| sqlx::Error::Protocol(error.to_string()))
}

fn author_label(authors: &[AuthorCredit]) -> String {
    authors.iter().map(AuthorCredit::name).collect::<Vec<_>>().join(", ")
}

async fn upsert_typed_state(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, change: &StateMutation) -> Result<(), sqlx::Error> {
    let (changed_at, actor, actor_seq, rank, event_id) = common(change, client_id);
    match &change.body {
        MutationBody::Directory { dir_id, value: directory } => {
            sqlx::query(UPSERT_DIRECTORY_SQL)
                .bind(library_id.as_str())
                .bind(dir_id.as_str())
                .bind(directory.parent_id().as_str())
                .bind(directory.name().as_str())
                .bind(directory.is_deleted())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::BookLifecycle {
            book_id,
            value: BookLifecycleState::Present { metadata, added_at },
        } => {
            sqlx::query(UPSERT_BOOK_PRESENT_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(&metadata.title)
                .bind(author_label(&metadata.authors))
                .bind(encoded_authors(&metadata.authors)?)
                .bind(&metadata.description)
                .bind(encoded_publication(&metadata.publication)?)
                .bind(added_at.storage_i64())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::BookLifecycle { book_id, value: BookLifecycleState::Deleted } => {
            sqlx::query(UPSERT_BOOK_DELETED_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::Placement {
            dir_id,
            file_name,
            value: PlacementState::Present { book_id, content_hash },
        } => {
            sqlx::query(UPSERT_PLACEMENT_PRESENT_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(dir_id.as_str())
                .bind(file_name.as_str())
                .bind(content_hash.as_str())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::Placement {
            dir_id,
            file_name,
            value: PlacementState::Vacant { removed_book_id: book_id },
        } => {
            sqlx::query(UPSERT_PLACEMENT_VACANT_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(dir_id.as_str())
                .bind(file_name.as_str())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::ReadingPosition {
            book_id,
            value: ReadingPositionState { location: cfi, progress },
        } => {
            sqlx::query(UPSERT_READING_POSITION_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(cfi)
                .bind(progress)
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::Annotation { annotation_id, value: annotation } => {
            let (anchor_kind, cfi, page_index, left, top, width, height, fallback) = match &annotation.anchor {
                AnnotationAnchor::EpubCfi { cfi } => {
                    ("epub_cfi", Some(cfi.as_str()), None, None, None, None, None, None)
                }
                AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => (
                    "pdf",
                    None,
                    Some(i64::from(*page_index)),
                    Some(rects.iter().map(|rect| rect.left).collect::<Vec<_>>()),
                    Some(rects.iter().map(|rect| rect.top).collect::<Vec<_>>()),
                    Some(rects.iter().map(|rect| rect.width).collect::<Vec<_>>()),
                    Some(rects.iter().map(|rect| rect.height).collect::<Vec<_>>()),
                    fallback_cfi.as_deref(),
                ),
            };
            let style = match annotation.style {
                AnnotationStyle::Highlight => "highlight",
                AnnotationStyle::Underline => "underline",
                AnnotationStyle::Squiggly => "squiggly",
                AnnotationStyle::Strikethrough => "strikethrough",
            };
            sqlx::query(UPSERT_ANNOTATION_SQL)
                .bind(library_id.as_str())
                .bind(annotation_id)
                .bind(annotation.book_id.canonical())
                .bind(anchor_kind)
                .bind(cfi)
                .bind(page_index)
                .bind(left)
                .bind(top)
                .bind(width)
                .bind(height)
                .bind(fallback)
                .bind(&annotation.exact_text)
                .bind(&annotation.prefix)
                .bind(&annotation.suffix)
                .bind(style)
                .bind(&annotation.color)
                .bind(&annotation.note)
                .bind(annotation.created_at.storage_i64())
                .bind(annotation.modified_at.storage_i64())
                .bind(annotation.deleted)
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::Bookmark { bookmark_id, value: bookmark } => {
            sqlx::query(UPSERT_BOOKMARK_SQL)
                .bind(library_id.as_str())
                .bind(bookmark_id)
                .bind(bookmark.book_id.canonical())
                .bind(&bookmark.cfi)
                .bind(&bookmark.label)
                .bind(bookmark.created_at.storage_i64())
                .bind(bookmark.modified_at.storage_i64())
                .bind(bookmark.deleted)
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::BookLink { link_id, value: link } => {
            sqlx::query(UPSERT_BOOK_LINK_SQL)
                .bind(library_id.as_str())
                .bind(link_id)
                .bind(link.source_book_id.canonical())
                .bind(link.target_book_id.canonical())
                .bind(&link.relation)
                .bind(&link.note)
                .bind(link.created_at.storage_i64())
                .bind(link.modified_at.storage_i64())
                .bind(link.deleted)
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::LastOpened { book_id, last_read } => {
            sqlx::query(UPSERT_LAST_OPENED_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(last_read.storage_i64())
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
        MutationBody::Metadata {
            book_id,
            value: BookMetadataState {
                title,
                authors,
                description,
                publication,
            },
        } => {
            sqlx::query(UPSERT_METADATA_SQL)
                .bind(library_id.as_str())
                .bind(book_id.canonical())
                .bind(title)
                .bind(author_label(authors))
                .bind(encoded_authors(authors)?)
                .bind(description)
                .bind(encoded_publication(publication)?)
                .bind(changed_at)
                .bind(actor)
                .bind(actor_seq)
                .bind(rank)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

async fn upsert_directory_batch(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, changes: &[&StateMutation]) -> Result<(), sqlx::Error> {
    if changes.is_empty() { return Ok(()); }
    let mut ids=Vec::with_capacity(changes.len());let mut parents=Vec::with_capacity(changes.len());let mut names=Vec::with_capacity(changes.len());let mut deleted=Vec::with_capacity(changes.len());let mut changed=Vec::with_capacity(changes.len());let mut seqs=Vec::with_capacity(changes.len());let mut ranks=Vec::with_capacity(changes.len());let mut events=Vec::with_capacity(changes.len());
    for change in changes { let MutationBody::Directory { dir_id,value }=&change.body else { unreachable!() };let (at,_,seq,rank,event)=common(change,client_id);ids.push(dir_id.as_str().to_owned());parents.push(value.parent_id().as_str().to_owned());names.push(value.name().as_str().to_owned());deleted.push(value.is_deleted());changed.push(at);seqs.push(seq);ranks.push(rank);events.push(event); }
    sqlx::query(UPSERT_DIRECTORY_BATCH_SQL).bind(library_id.as_str()).bind(client_id.as_str()).bind(ids).bind(parents).bind(names).bind(deleted).bind(changed).bind(seqs).bind(ranks).bind(events).execute(&mut **tx).await?;Ok(())
}

async fn upsert_book_lifecycle_batch(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, changes: &[&StateMutation]) -> Result<(), sqlx::Error> {
    if changes.is_empty() {
        return Ok(());
    }

    let mut books = Vec::with_capacity(changes.len());
    let mut titles = Vec::with_capacity(changes.len());
    let mut author_labels = Vec::with_capacity(changes.len());
    let mut author_payloads = Vec::with_capacity(changes.len());
    let mut descriptions = Vec::with_capacity(changes.len());
    let mut publications = Vec::with_capacity(changes.len());
    let mut added = Vec::with_capacity(changes.len());
    let mut deleted = Vec::with_capacity(changes.len());
    let mut changed = Vec::with_capacity(changes.len());
    let mut seqs = Vec::with_capacity(changes.len());
    let mut ranks = Vec::with_capacity(changes.len());
    let mut events = Vec::with_capacity(changes.len());

    for change in changes {
        let MutationBody::BookLifecycle { book_id, value } = &change.body else {
            unreachable!()
        };
        let (title, authors, description, publication, added_at, is_deleted) = match value {
            BookLifecycleState::Present { metadata, added_at } => (
                Some(metadata.title.clone()),
                metadata.authors.clone(),
                metadata.description.clone(),
                encoded_publication(&metadata.publication)?,
                Some(added_at.storage_i64()),
                false,
            ),
            BookLifecycleState::Deleted => (
                None,
                Vec::new(),
                String::new(),
                Vec::new(),
                None,
                true,
            ),
        };
        let (at, _, seq, rank, event) = common(change, client_id);

        books.push(book_id.canonical());
        titles.push(title);
        author_labels.push(Some(author_label(&authors)));
        author_payloads.push(encoded_authors(&authors)?);
        descriptions.push(description);
        publications.push(publication);
        added.push(added_at);
        deleted.push(is_deleted);
        changed.push(at);
        seqs.push(seq);
        ranks.push(rank);
        events.push(event);
    }

    sqlx::query(UPSERT_BOOK_LIFECYCLE_BATCH_SQL)
        .bind(library_id.as_str())
        .bind(client_id.as_str())
        .bind(books)
        .bind(titles)
        .bind(author_labels)
        .bind(author_payloads)
        .bind(descriptions)
        .bind(publications)
        .bind(added)
        .bind(deleted)
        .bind(changed)
        .bind(seqs)
        .bind(ranks)
        .bind(events)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn upsert_placement_batch(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, changes: &[&StateMutation]) -> Result<Vec<PlacementHashDelta>, sqlx::Error> {
    if changes.is_empty() { return Ok(Vec::new()); }
    let mut books=Vec::with_capacity(changes.len());let mut dirs=Vec::with_capacity(changes.len());let mut names=Vec::with_capacity(changes.len());let mut hashes=Vec::with_capacity(changes.len());let mut present=Vec::with_capacity(changes.len());let mut changed=Vec::with_capacity(changes.len());let mut seqs=Vec::with_capacity(changes.len());let mut ranks=Vec::with_capacity(changes.len());let mut events=Vec::with_capacity(changes.len());
    for change in changes { let MutationBody::Placement { dir_id,file_name,value }=&change.body else { unreachable!() };let (book,hash,is_present)=match value{PlacementState::Present{book_id,content_hash}=>(*book_id,content_hash.as_str().to_owned(),true),PlacementState::Vacant{removed_book_id}=>(*removed_book_id,String::new(),false)};let (at,_,seq,rank,event)=common(change,client_id);books.push(book.canonical());dirs.push(dir_id.as_str().to_owned());names.push(file_name.as_str().to_owned());hashes.push(hash);present.push(is_present);changed.push(at);seqs.push(seq);ranks.push(rank);events.push(event); }
    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(UPSERT_PLACEMENT_BATCH_SQL).bind(library_id.as_str()).bind(client_id.as_str()).bind(books).bind(dirs).bind(names).bind(hashes).bind(present).bind(changed).bind(seqs).bind(ranks).bind(events).fetch_all(&mut **tx).await?;
    rows.into_iter().map(|(previous,current)| Ok(PlacementHashDelta {
        previous: previous.map(|hash| ContentHash::parse(&hash).map_err(|error| sqlx::Error::Decode(Box::new(error)))).transpose()?,
        current: current.map(|hash| ContentHash::parse(&hash).map_err(|error| sqlx::Error::Decode(Box::new(error)))).transpose()?,
    })).collect()
}

async fn upsert_reading_position_batch(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, changes: &[&StateMutation]) -> Result<(), sqlx::Error> {
    if changes.is_empty() { return Ok(()); }
    let mut books=Vec::with_capacity(changes.len());let mut positions=Vec::with_capacity(changes.len());let mut progress=Vec::with_capacity(changes.len());let mut changed=Vec::with_capacity(changes.len());let mut seqs=Vec::with_capacity(changes.len());let mut ranks=Vec::with_capacity(changes.len());let mut events=Vec::with_capacity(changes.len());
    for change in changes { let MutationBody::ReadingPosition { book_id,value }=&change.body else { unreachable!() };let (at,_,seq,rank,event)=common(change,client_id);books.push(book_id.canonical());positions.push(value.location.clone());progress.push(value.progress);changed.push(at);seqs.push(seq);ranks.push(rank);events.push(event); }
    sqlx::query(UPSERT_READING_POSITION_BATCH_SQL).bind(library_id.as_str()).bind(client_id.as_str()).bind(books).bind(positions).bind(progress).bind(changed).bind(seqs).bind(ranks).bind(events).execute(&mut **tx).await?;Ok(())
}

async fn upsert_metadata_batch(tx: &mut Transaction<'_, Postgres>, library_id: &LibraryId, client_id: &ClientId, changes: &[&StateMutation]) -> Result<(), sqlx::Error> {
    if changes.is_empty() {
        return Ok(());
    }

    let mut books = Vec::with_capacity(changes.len());
    let mut titles = Vec::with_capacity(changes.len());
    let mut author_labels = Vec::with_capacity(changes.len());
    let mut author_payloads = Vec::with_capacity(changes.len());
    let mut descriptions = Vec::with_capacity(changes.len());
    let mut publications = Vec::with_capacity(changes.len());
    let mut changed = Vec::with_capacity(changes.len());
    let mut seqs = Vec::with_capacity(changes.len());
    let mut ranks = Vec::with_capacity(changes.len());
    let mut events = Vec::with_capacity(changes.len());

    for change in changes {
        let MutationBody::Metadata { book_id, value } = &change.body else {
            unreachable!()
        };
        let (at, _, seq, rank, event) = common(change, client_id);

        books.push(book_id.canonical());
        titles.push(value.title.clone());
        author_labels.push(author_label(&value.authors));
        author_payloads.push(encoded_authors(&value.authors)?);
        descriptions.push(value.description.clone());
        publications.push(encoded_publication(&value.publication)?);
        changed.push(at);
        seqs.push(seq);
        ranks.push(rank);
        events.push(event);
    }

    sqlx::query(UPSERT_METADATA_BATCH_SQL)
        .bind(library_id.as_str())
        .bind(client_id.as_str())
        .bind(books)
        .bind(titles)
        .bind(author_labels)
        .bind(author_payloads)
        .bind(descriptions)
        .bind(publications)
        .bind(changed)
        .bind(seqs)
        .bind(ranks)
        .bind(events)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn insert_changes(tx:&mut Transaction<'_,Postgres>,_user_id:&str,library_id:&LibraryId,client_id:&ClientId,changes:&[StateMutation])->Result<InsertChangesResult,sqlx::Error>{
    let server_now=server_now_ms();let mut rejected=Vec::new();let mut candidates=Vec::new();
    for change in changes { if let Some(reason)=storage_rejection(change,server_now){rejected.push(MutationRejection{mutation_id:change.mutation_id,reason});}else{candidates.push(change);} }

    let provided_dirs=candidates.iter().filter_map(|change|match &change.body{MutationBody::Directory{dir_id,..}=>Some(dir_id.as_str().to_owned()),_=>None}).collect::<std::collections::HashSet<_>>();
    let provided_books=candidates.iter().filter_map(|change|match &change.body{MutationBody::BookLifecycle{book_id,..}=>Some(book_id.canonical()),MutationBody::Placement{value:PlacementState::Present{book_id,..},..}=>Some(book_id.canonical()),MutationBody::Placement{value:PlacementState::Vacant{removed_book_id},..}=>Some(removed_book_id.canonical()),_=>None}).collect::<std::collections::HashSet<_>>();
    let required_dirs=candidates.iter().filter_map(|change|required_directory(&change.body)).filter(|dir|dir.as_str()!="__root__"&&!provided_dirs.contains(dir.as_str())).map(|dir|dir.as_str().to_owned()).collect::<std::collections::HashSet<_>>();
    let required_book_ids=candidates.iter().flat_map(|change|required_books(&change.body).into_iter().flatten()).map(|book|book.canonical()).filter(|book|!provided_books.contains(book)).collect::<std::collections::HashSet<_>>();
    let required_dir_values=required_dirs.iter().cloned().collect::<Vec<_>>();let required_book_values=required_book_ids.iter().cloned().collect::<Vec<_>>();
    let rows=sqlx::query(EXISTING_DEPENDENCIES_SQL).bind(library_id.as_str()).bind(&required_dir_values).bind(&required_book_values).fetch_all(&mut **tx).await?;
    let mut existing_dirs=std::collections::HashSet::new();let mut existing_books=std::collections::HashSet::new();
    for row in rows { let kind:String=row.try_get("kind")?;let id:String=row.try_get("id")?;if kind=="directory"{existing_dirs.insert(id);}else{existing_books.insert(id);} }

    let mut accepted=Vec::new();
    for change in candidates {
        let missing_dir=required_directory(&change.body).is_some_and(|dir|dir.as_str()!="__root__"&&!provided_dirs.contains(dir.as_str())&&!existing_dirs.contains(dir.as_str()));
        let missing_book=required_books(&change.body).into_iter().flatten().map(|book|book.canonical()).any(|book|!provided_books.contains(&book)&&!existing_books.contains(&book));
        if missing_dir||missing_book{rejected.push(MutationRejection{mutation_id:change.mutation_id,reason:MutationRejectionReason::MissingDependency});}else{accepted.push(change);}
    }

    let storage_changes=sync_common::reduce_same_replica_mutations(accepted.iter().copied());
    let directories=storage_changes.iter().copied().filter(|change|matches!(change.body,MutationBody::Directory{..})).collect::<Vec<_>>();
    let lifecycles=storage_changes.iter().copied().filter(|change|matches!(change.body,MutationBody::BookLifecycle{..})).collect::<Vec<_>>();
    let placements=storage_changes.iter().copied().filter(|change|matches!(change.body,MutationBody::Placement{..})).collect::<Vec<_>>();
    let readings=storage_changes.iter().copied().filter(|change|matches!(change.body,MutationBody::ReadingPosition{..})).collect::<Vec<_>>();
    let metadata=storage_changes.iter().copied().filter(|change|matches!(change.body,MutationBody::Metadata{..})).collect::<Vec<_>>();
    upsert_directory_batch(tx,library_id,client_id,&directories).await?;upsert_book_lifecycle_batch(tx,library_id,client_id,&lifecycles).await?;
    let mut placement_counts=std::collections::HashMap::new();
    for change in &placements { let MutationBody::Placement{dir_id,file_name,..}=&change.body else{unreachable!()};*placement_counts.entry((dir_id.as_str(),file_name.as_str())).or_insert(0_usize)+=1; }
    let unique_placements=placements.iter().copied().filter(|change|{let MutationBody::Placement{dir_id,file_name,..}=&change.body else{unreachable!()};placement_counts[&(dir_id.as_str(),file_name.as_str())]==1}).collect::<Vec<_>>();
    let mut placement_deltas=upsert_placement_batch(tx,library_id,client_id,&unique_placements).await?;
    let mut repeated=std::collections::BTreeMap::<(String,String),Vec<&StateMutation>>::new();
    for change in placements { let MutationBody::Placement{dir_id,file_name,..}=&change.body else{unreachable!()};if placement_counts[&(dir_id.as_str(),file_name.as_str())]>1{repeated.entry((dir_id.as_str().to_owned(),file_name.as_str().to_owned())).or_default().push(change);} }
    for changes in repeated.values_mut(){changes.sort_by_key(|change|(change.replica_seq,change.mutation_id));for change in changes.iter().copied(){placement_deltas.extend(upsert_placement_batch(tx,library_id,client_id,&[change]).await?);}}
    upsert_reading_position_batch(tx,library_id,client_id,&readings).await?;
    for change in storage_changes.iter().copied().filter(|change|!matches!(change.body,MutationBody::Directory{..}|MutationBody::BookLifecycle{..}|MutationBody::Placement{..}|MutationBody::ReadingPosition{..}|MutationBody::Metadata{..})){upsert_typed_state(tx,library_id,client_id,change).await?;}
    upsert_metadata_batch(tx,library_id,client_id,&metadata).await?;
    Ok(InsertChangesResult{accepted:accepted.into_iter().map(|change|change.mutation_id).collect(),rejected,placement_deltas})
}

fn required_directory(body:&MutationBody)->Option<&DirId>{match body{MutationBody::Directory{value,..}=>Some(value.parent_id()),MutationBody::Placement{dir_id,..}=>Some(dir_id),_=>None}}
fn required_books(body:&MutationBody)->[Option<BookId>;2]{match body{MutationBody::BookLink{value,..}=>[Some(value.source_book_id),Some(value.target_book_id)],_=>[None,None]}}

pub(super) enum AtomicSingletonInsert{Unauthorized,Handled}
pub(super) async fn insert_authorized_singleton(pool:&PgPool,user_id:&str,library_id:&LibraryId,client_id:&ClientId,change:&StateMutation)->Result<AtomicSingletonInsert,sqlx::Error>{
    let MutationBody::ReadingPosition{book_id,value:ReadingPositionState { location:cfi,progress }}=&change.body else{unreachable!()};let (changed_at,actor,seq,rank,id)=common(change,client_id);
    let (authorized,_):(bool,bool)=sqlx::query_as(INSERT_AUTHORIZED_READING_POSITION_SQL).bind(library_id.as_str()).bind(user_id).bind(book_id.canonical()).bind(cfi).bind(progress).bind(changed_at).bind(actor).bind(seq).bind(rank).bind(id).fetch_one(pool).await?;Ok(if authorized{AtomicSingletonInsert::Handled}else{AtomicSingletonInsert::Unauthorized})
}

#[derive(Debug,thiserror::Error)]pub(super)enum FetchChangesError{#[error(transparent)]Database(#[from]sqlx::Error),#[error("client cursor is ahead of server state")]CursorAhead,#[error("corrupt stored state at revision {revision}: {reason}")]CorruptStoredEvent{revision:i64,reason:&'static str}}
pub(super)struct FetchChangesPage{pub changes:Vec<ServerMutation>,pub has_more:bool}
pub(super)enum FetchChangesResult{Unauthorized,Page(FetchChangesPage)}

fn at<'a, T>(values: &'a [T], index: usize) -> Result<&'a T, sqlx::Error> {
    values
        .get(index)
        .ok_or_else(|| sqlx::Error::Decode("typed sync projection is missing a field".into()))
}

fn millis(value: i64) -> Result<UnixMillis, sqlx::Error> {
    let value = u64::try_from(value)
        .map_err(|_| sqlx::Error::Decode("negative Unix-millisecond timestamp".into()))?;
    UnixMillis::new(value).map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

fn decode_operation(
    event_type: &str,
    entity: String,
    texts: Vec<String>,
    ints: Vec<i64>,
    reals: Vec<f32>,
    bools: Vec<bool>,
    left: Vec<f32>,
    top: Vec<f32>,
    width: Vec<f32>,
    height: Vec<f32>,
    authors_payload: Vec<u8>,
    publication_payload: Vec<u8>,
) -> Result<MutationBody, sqlx::Error> {
    let book = || {
        BookId::parse(&entity).map_err(|error| sqlx::Error::Decode(Box::new(error)))
    };
    let migrated_credit = |index: usize, name: String, sort_name: Option<String>| {
        let seed = format!("{event_type}:{entity}:{index}:{name}");
        AuthorCredit::with_id(
            AuthorId::from_migration_seed(seed.as_bytes()),
            name,
            sort_name,
        )
        .map_err(|error| sync_common::wire::WireError::Decode(error.to_string()))
    };
    let decoded_authors = || {
        sync_common::wire::decode::<Vec<AuthorCredit>>(&authors_payload, 64 * 1024)
            .or_else(|_| {
                sync_common::wire::decode::<Vec<LegacyAuthorCredit>>(
                    &authors_payload,
                    64 * 1024,
                )
                .and_then(|legacy| {
                    legacy
                        .into_iter()
                        .enumerate()
                        .map(|(index, credit)| {
                            migrated_credit(index, credit.name, credit.sort_name)
                        })
                        .collect()
                })
                .or_else(|_| {
                    sync_common::wire::decode::<Vec<String>>(
                        &authors_payload,
                        64 * 1024,
                    )
                    .and_then(|legacy| {
                        legacy
                            .into_iter()
                            .enumerate()
                            .map(|(index, name)| migrated_credit(index, name, None))
                            .collect()
                    })
                })
            })
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))
    };
    let decoded_publication = || {
        if publication_payload.is_empty() {
            Ok(PublicationMetadata::default())
        } else {
            sync_common::wire::decode(
                &publication_payload,
                sync_common::wire::MAX_DECODED_REQUEST_BYTES,
            )
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))
        }
    };

    let operation = match event_type {
        "dir_state" => {
            let dir_id = DirId::parse(&entity)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            let parent_id = DirId::parse(at(&texts, 0)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            let name = FileName::parse(at(&texts, 1)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            let value = if *at(&bools, 0)? {
                DirectoryState::Deleted { parent_id, name }
            } else {
                DirectoryState::Present { parent_id, name }
            };
            MutationBody::Directory { dir_id, value }
        }
        "book_added" => {
            let mut authors = decoded_authors()?;
            if authors.is_empty() && !at(&texts, 1)?.is_empty() {
                authors.push(
                    migrated_credit(0, at(&texts, 1)?.clone(), None)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                );
            }
            MutationBody::BookLifecycle {
                book_id: book()?,
                value: BookLifecycleState::Present {
                    metadata: BookMetadataState {
                        title: at(&texts, 0)?.clone(),
                        authors,
                        description: at(&texts, 2)?.clone(),
                        publication: decoded_publication()?,
                    },
                    added_at: millis(*at(&ints, 0)?)?,
                },
            }
        }
        "book_deleted" => MutationBody::BookLifecycle {
            book_id: book()?,
            value: BookLifecycleState::Deleted,
        },
        "book_dir_added" => MutationBody::Placement {
            dir_id: DirId::parse(at(&texts, 0)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            file_name: FileName::parse(at(&texts, 1)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            value: PlacementState::Present {
                book_id: book()?,
                content_hash: ContentHash::parse(at(&texts, 2)?)
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            },
        },
        "book_dir_removed" => MutationBody::Placement {
            dir_id: DirId::parse(at(&texts, 0)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            file_name: FileName::parse(at(&texts, 1)?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            value: PlacementState::Vacant {
                removed_book_id: book()?,
            },
        },
        "reading_position_updated" => MutationBody::ReadingPosition {
            book_id: book()?,
            value: ReadingPositionState {
                location: at(&texts, 0)?.clone(),
                progress: *at(&reals, 0)?,
            },
        },
        "annotation_state" => {
            let anchor = if at(&texts, 1)? == "epub_cfi" {
                AnnotationAnchor::epub_cfi(at(&texts, 2)?.clone())
                    .ok_or_else(|| sqlx::Error::Decode("invalid EPUB annotation anchor".into()))?
            } else {
                if left.len() != top.len()
                    || left.len() != width.len()
                    || left.len() != height.len()
                {
                    return Err(sqlx::Error::Decode(
                        "mismatched annotation rectangles".into(),
                    ));
                }
                let rects = left
                    .into_iter()
                    .zip(top)
                    .zip(width)
                    .zip(height)
                    .map(|(((left, top), width), height)| {
                        PdfAnnotationRect::new(left, top, width, height).ok_or_else(|| {
                            sqlx::Error::Decode("invalid annotation rectangle".into())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let page_index = u32::try_from(*at(&ints, 0)?)
                    .map_err(|_| sqlx::Error::Decode("invalid annotation page".into()))?;
                let fallback_cfi = if *at(&bools, 1)? {
                    Some(at(&texts, 3)?.clone())
                } else {
                    None
                };
                AnnotationAnchor::pdf(PdfAnnotationAnchor {
                    page_index,
                    rects,
                    fallback_cfi,
                })
                .ok_or_else(|| sqlx::Error::Decode("invalid PDF annotation anchor".into()))?
            };
            let style = match at(&texts, 7)?.as_str() {
                "highlight" => AnnotationStyle::Highlight,
                "underline" => AnnotationStyle::Underline,
                "squiggly" => AnnotationStyle::Squiggly,
                "strikethrough" => AnnotationStyle::Strikethrough,
                _ => return Err(sqlx::Error::Decode("invalid annotation style".into())),
            };
            MutationBody::Annotation {
                annotation_id: entity,
                value: AnnotationState {
                    book_id: BookId::parse(at(&texts, 0)?)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                    anchor,
                    exact_text: at(&texts, 4)?.clone(),
                    prefix: if *at(&bools, 2)? {
                        Some(at(&texts, 5)?.clone())
                    } else {
                        None
                    },
                    suffix: if *at(&bools, 3)? {
                        Some(at(&texts, 6)?.clone())
                    } else {
                        None
                    },
                    style,
                    color: at(&texts, 8)?.clone(),
                    note: at(&texts, 9)?.clone(),
                    created_at: millis(*at(&ints, 1)?)?,
                    modified_at: millis(*at(&ints, 2)?)?,
                    deleted: *at(&bools, 0)?,
                },
            }
        }
        "bookmark_state" => MutationBody::Bookmark {
            bookmark_id: entity,
            value: BookmarkState {
                book_id: BookId::parse(at(&texts, 0)?)
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                cfi: at(&texts, 1)?.clone(),
                label: at(&texts, 2)?.clone(),
                created_at: millis(*at(&ints, 0)?)?,
                modified_at: millis(*at(&ints, 1)?)?,
                deleted: *at(&bools, 0)?,
            },
        },
        "book_link_state" => MutationBody::BookLink {
            link_id: entity,
            value: BookLinkState {
                source_book_id: BookId::parse(at(&texts, 0)?)
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                target_book_id: BookId::parse(at(&texts, 1)?)
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                relation: at(&texts, 2)?.clone(),
                note: at(&texts, 3)?.clone(),
                created_at: millis(*at(&ints, 0)?)?,
                modified_at: millis(*at(&ints, 1)?)?,
                deleted: *at(&bools, 0)?,
            },
        },
        "book_opened" => MutationBody::LastOpened {
            book_id: book()?,
            last_read: millis(*at(&ints, 0)?)?,
        },
        "book_metadata_updated" => {
            let mut authors = decoded_authors()?;
            if authors.is_empty() && !at(&texts, 1)?.is_empty() {
                authors.push(
                    migrated_credit(0, at(&texts, 1)?.clone(), None)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                );
            }
            MutationBody::Metadata {
                book_id: book()?,
                value: BookMetadataState {
                    title: at(&texts, 0)?.clone(),
                    authors,
                    description: at(&texts, 2)?.clone(),
                    publication: decoded_publication()?,
                },
            }
        }
        other => {
            return Err(sqlx::Error::Decode(
                format!("unsupported typed state {other}").into(),
            ));
        }
    };
    operation
        .validate()
        .map_err(|_| sqlx::Error::Decode("typed state violates protocol validation".into()))?;
    Ok(operation)
}

fn cursor(value: Option<LibraryRevision>) -> i64 {
    value.map(|revision| i64::try_from(revision.get()).unwrap_or(i64::MAX)).unwrap_or(0)
}

fn cursor_is_ahead(value: Option<LibraryRevision>, current_revision: i64) -> bool {
    cursor(value) > current_revision
}
async fn fetch_projected_page<'e, E: sqlx::Executor<'e, Database = Postgres>>(
    executor: E,
    statement: &str,
    library: &LibraryId,
    since: Option<LibraryRevision>,
) -> Result<FetchChangesPage, FetchChangesError> {
    let rows = sqlx::query(statement)
        .bind(library.as_str())
        .bind(cursor(since))
        .bind((MAX_PULL_CHANGES + 1) as i64)
        .fetch_all(executor)
        .await?;
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let raw: i64 = row.try_get("server_seq")?;
        let corrupt = |reason| FetchChangesError::CorruptStoredEvent { revision: raw, reason };
        let event_type: String = row.try_get("event_type")?;
        let body = decode_operation(
            &event_type,
            row.try_get("entity_id")?,
            row.try_get("texts")?,
            row.try_get("integers")?,
            row.try_get("reals")?,
            row.try_get("booleans")?,
            row.try_get("rect_left")?,
            row.try_get("rect_top")?,
            row.try_get("rect_width")?,
            row.try_get("rect_height")?,
            row.try_get("authors_payload")?,
            row.try_get("publication_payload")?,
        )?;
        let changed_at = millis(row.try_get::<i64, _>("changed_at")?).map_err(|_| corrupt("invalid timestamp"))?;
        let replica_id = ClientId::parse(&row.try_get::<String, _>("client_id")?).map_err(|_| corrupt("invalid replica"))?;
        let replica_seq = ReplicaSeq::new(u64::try_from(row.try_get::<i64, _>("client_seq")?).map_err(|_| corrupt("negative replica sequence"))?)
            .map_err(|_| corrupt("invalid replica sequence"))?;
        let revision = LibraryRevision::new(u64::try_from(raw).map_err(|_| corrupt("negative library revision"))?)
            .map_err(|_| corrupt("invalid library revision"))?;
        let mutation_id = MutationId::parse(&row.try_get::<String, _>("event_id")?).map_err(|_| corrupt("invalid mutation identity"))?;
        changes.push(ServerMutation { mutation: StateMutation { mutation_id, body, changed_at, replica_seq }, replica_id, revision });
    }
    let has_more = changes.len() > MAX_PULL_CHANGES;
    changes.truncate(MAX_PULL_CHANGES);
    Ok(FetchChangesPage { changes, has_more })
}

async fn fetch_page<'e, E: sqlx::Executor<'e, Database = Postgres>>(executor: E, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_projected_page(executor, FETCH_PAGE_SQL, library, since).await
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PullRoute { Empty, ReadingOnly, Full }

fn pull_route(since: Option<LibraryRevision>, reading_revision: i64, other_revision: i64) -> PullRoute {
    let since = cursor(since);
    if reading_revision <= since && other_revision <= since {
        PullRoute::Empty
    } else if other_revision <= since {
        PullRoute::ReadingOnly
    } else {
        PullRoute::Full
    }
}

async fn fetch_reading_position_page(pool: &PgPool, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_projected_page(pool, FETCH_READING_POSITION_PAGE_SQL, library, since).await
}

pub(super) async fn fetch_changes(pool: &PgPool, user: &str, library: &LibraryId, _client: &ClientId, since: Option<LibraryRevision>) -> Result<FetchChangesResult, FetchChangesError> {
    let heads = sqlx::query(GET_PULL_HEADS_SQL)
        .bind(library.as_str())
        .bind(user)
        .fetch_optional(pool)
        .await?;
    let Some(heads) = heads else { return Ok(FetchChangesResult::Unauthorized) };
    let reading_revision: i64 = heads.try_get("reading_revision")?;
    let other_revision: i64 = heads.try_get("other_revision")?;
    if cursor_is_ahead(since, std::cmp::max(reading_revision, other_revision)) {
        return Err(FetchChangesError::CursorAhead);
    }
    let page = match pull_route(since, reading_revision, other_revision) {
        PullRoute::Empty => FetchChangesPage { changes: Vec::new(), has_more: false },
        PullRoute::ReadingOnly => fetch_reading_position_page(pool, library, since).await?,
        PullRoute::Full => fetch_page(pool, library, since).await?,
    };
    Ok(FetchChangesResult::Page(page))
}

pub(super) async fn validate_owned_cursor(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: Option<LibraryRevision>) -> Result<(), FetchChangesError> {
    let current: i64 = sqlx::query_scalar(VALIDATE_OWNED_CURSOR_SQL).bind(library.as_str()).fetch_one(&mut **tx).await?;
    if cursor_is_ahead(since, current) {
        return Err(FetchChangesError::CursorAhead);
    }
    Ok(())
}

pub(super) async fn fetch_owned_changes(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_page(&mut **tx, library, since).await
}

pub(super)enum AtomicSingletonExchange{Unauthorized,CursorAhead,Handled(FetchChangesPage)}
pub(super) async fn exchange_authorized_singleton(pool: &PgPool, user_id: &str, library: &LibraryId, client: &ClientId, change: &StateMutation, since: Option<LibraryRevision>) -> Result<AtomicSingletonExchange, FetchChangesError> {
    let MutationBody::ReadingPosition { book_id, value: ReadingPositionState { location: cfi, progress } } = &change.body else { unreachable!() };
    let (changed_at, actor, seq, rank, id) = common(change, client);
    let row = sqlx::query(EXCHANGE_AUTHORIZED_READING_POSITION_SQL)
        .bind(library.as_str())
        .bind(user_id)
        .bind(book_id.canonical())
        .bind(cfi)
        .bind(progress)
        .bind(changed_at)
        .bind(actor)
        .bind(seq)
        .bind(rank)
        .bind(id)
        .bind(cursor(since))
        .fetch_one(pool)
        .await?;
    if !row.try_get::<bool, _>("authorized")? {
        return Ok(AtomicSingletonExchange::Unauthorized);
    }
    if row.try_get::<bool, _>("cursor_ahead")? {
        return Ok(AtomicSingletonExchange::CursorAhead);
    }

    let changed_revision = row.try_get::<Option<i64>, _>("server_seq")?;
    if row.try_get::<bool, _>("caught_up")? {
        let changes = if let Some(raw) = changed_revision {
            vec![ServerMutation {
                mutation: change.clone(),
                replica_id: ClientId::parse(client.as_str()).map_err(|_| FetchChangesError::CorruptStoredEvent { revision: raw, reason: "invalid replica" })?,
                revision: LibraryRevision::new(raw as u64).map_err(|_| FetchChangesError::CorruptStoredEvent { revision: raw, reason: "invalid revision" })?,
            }]
        } else {
            Vec::new()
        };
        return Ok(AtomicSingletonExchange::Handled(FetchChangesPage { changes, has_more: false }));
    }

    let prior_reading = row.try_get::<Option<i64>, _>("reading_revision")?.unwrap_or(0);
    let prior_other = row.try_get::<Option<i64>, _>("other_revision")?.unwrap_or(0);
    let reading_after_write = changed_revision.map_or(prior_reading, |revision| std::cmp::max(prior_reading, revision));
    let page = match pull_route(since, reading_after_write, prior_other) {
        PullRoute::Empty => FetchChangesPage { changes: Vec::new(), has_more: false },
        PullRoute::ReadingOnly => fetch_reading_position_page(pool, library, since).await?,
        PullRoute::Full => fetch_page(pool, library, since).await?,
    };
    Ok(AtomicSingletonExchange::Handled(page))
}

pub(super)async fn live_assets(tx:&mut Transaction<'_,Postgres>)->Result<LiveAssets,sqlx::Error>{let rows:Vec<(String,String)>=sqlx::query_as(LIVE_ASSETS_SQL).fetch_all(&mut**tx).await?;let mut books=std::collections::HashSet::new();let mut thumbnails=std::collections::HashSet::new();for(user_id,raw)in rows{if let Ok(hash)=ContentHash::parse(&raw){books.insert(hash.clone());thumbnails.insert(UserThumbnail{user_id,content_hash:hash});}}Ok(LiveAssets::new(books,thumbnails))}

#[cfg(test)]
mod tests {
    use super::{
        cursor_is_ahead, decode_operation, pull_route, required_books,
        required_directory, storage_rejection, PullRoute,
        UPSERT_READING_POSITION_SQL,
    };
    use sync_common::{
        AuthorCredit, BookId, BookLifecycleState, DirId, DirectoryState,
        FileName, LanguageTag, LibraryRevision, MutationBody, MutationId,
        MutationRejectionReason, PublicationIdentifier,
        PublicationIdentifierScheme, PublicationMetadata, PublisherCredit,
        ReadingPositionState, ReplicaSeq, StateMutation,
        MAX_LWW_FUTURE_SKEW_MS,
    };

    #[test]
    fn projected_lifecycle_decoding_preserves_publication_metadata() {
        let authors = vec![AuthorCredit::from_name("Ursula K. Le Guin").unwrap()];
        let publication = PublicationMetadata {
            identifiers: vec![PublicationIdentifier::new(
                "9781234567897",
                PublicationIdentifierScheme::Isbn,
                Vec::new(),
            )
            .unwrap()],
            publishers: vec![PublisherCredit::new("North Wind Press").unwrap()],
            languages: vec![LanguageTag::parse("sv-SE").unwrap()],
            rights: vec!["Copyright example".to_owned()],
            ..PublicationMetadata::default()
        };
        let decoded = decode_operation(
            "book_added",
            BookId::new(7).canonical(),
            vec![
                "The Dispossessed".to_owned(),
                "Ursula K. Le Guin".to_owned(),
                "A description".to_owned(),
            ],
            vec![1_700_000_000_000],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            sync_common::wire::encode(&authors).unwrap(),
            sync_common::wire::encode(&publication).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            decoded,
            MutationBody::BookLifecycle {
                book_id,
                value: BookLifecycleState::Present { metadata, .. },
            } if book_id == BookId::new(7)
                && metadata.authors == authors
                && metadata.publication == publication
        ));
    }

    #[test]
    fn pull_routing_preserves_the_global_cursor_boundary() {
        let cursor = LibraryRevision::new(10).ok();
        assert_eq!(pull_route(cursor, 10, 10), PullRoute::Empty);
        assert_eq!(pull_route(cursor, 11, 10), PullRoute::ReadingOnly);
        assert_eq!(pull_route(cursor, 10, 11), PullRoute::Full);
        assert_eq!(pull_route(cursor, 12, 11), PullRoute::Full);
        assert_eq!(pull_route(None, 1, 0), PullRoute::ReadingOnly);
        assert_eq!(pull_route(None, 0, 1), PullRoute::Full);
    }

    #[test]
    fn a_cursor_may_equal_but_never_exceed_the_server_head() {
        assert!(!cursor_is_ahead(None, 0));
        assert!(!cursor_is_ahead(LibraryRevision::new(10).ok(), 10));
        assert!(cursor_is_ahead(LibraryRevision::new(11).ok(), 10));
    }

    #[test]
    fn relational_dependencies_are_explicit() {
        let directory = DirId::parse("parent").unwrap();
        let body = MutationBody::Directory { dir_id: DirId::parse("child").unwrap(), value: DirectoryState::Present { parent_id: directory.clone(), name: FileName::parse("Child").unwrap() } };
        assert_eq!(required_directory(&body), Some(&directory));
        assert_eq!(required_books(&MutationBody::BookLifecycle { book_id: BookId::new(1), value: BookLifecycleState::Deleted }), [None, None]);
    }

    fn reading_mutation(changed_at: u64) -> StateMutation {
        StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/4)".into(), progress: 25.0 } },
            changed_at: sync_common::UnixMillis::new(changed_at).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        }
    }

    #[test]
    fn server_admission_rejects_future_wall_clock() {
        let server_now = 1_000_000;
        assert_eq!(
            storage_rejection(&reading_mutation(server_now + MAX_LWW_FUTURE_SKEW_MS + 1), server_now),
            Some(MutationRejectionReason::TimestampTooFarFuture),
        );
        assert_eq!(storage_rejection(&reading_mutation(server_now), server_now), None);
    }

    #[test]
    fn postgres_winner_order_begins_with_wall_clock_time() {
        let comparison = UPSERT_READING_POSITION_SQL.split("WHERE (").nth(1).expect("LWW predicate");
        assert!(comparison.trim_start().starts_with("EXCLUDED.changed_at,"));
        assert!(!UPSERT_READING_POSITION_SQL.contains("hlc_"));
    }
}
