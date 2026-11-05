//! Owner-committed pages for the storage-independent sync engine.
use crate::engine::PreparedSync;
use crate::engine::StoreFuture;
use library_database::{DatabaseClient, DatabaseError, OutboxScan};
use library_model::LibraryUpdate;
use sync_common::{PullStateResponse, ReplicaSeq, MAX_PUSH_MUTATIONS};

use library_replica::{LibraryStateEntry, StatePublication};
use library_services::events::{LibraryEvent, LibraryEventRouter};
use library_services::reading_positions::ReadingPositionPolicy;

/// One scan pins both the durable queue bounds and the owner's policy revision.
pub(crate) struct PublicationOutbox {
    scan: OutboxScan,
    positions: ReadingPositionPolicy,
}

/// Native and browser storage owners prepare the same bounded first page.
pub(crate) async fn prepare_sync(database: &DatabaseClient, events: Option<&LibraryEventRouter>) -> Result<PreparedSync<PublicationOutbox>, DatabaseError> {
    let mut trace = sync_transport::PerformanceTrace::new("sync_prepare", "check_recovery");
    if database.sync_cursor_recovery_pending().await? {
        trace.finish(true);
        return Ok(PreparedSync::RecoveryPending);
    }
    trace.phase("read_cursor");
    let cursor = database.sync_pull_cursor().await?;
    trace.phase("prepare_outbox");
    let mut outbox = prepare_outbox(database, events).await?;
    trace.phase("first_outbox_page");
    let mutations = next_outbox_page(database, events, &mut outbox).await?;
    trace.finish(true);
    Ok(PreparedSync::Ready { cursor, outbox, mutations })
}

pub(crate) async fn prepare_outbox(database: &DatabaseClient, events: Option<&LibraryEventRouter>) -> Result<PublicationOutbox, DatabaseError> {
    let positions = events.map(|events| events.reading_positions.borrow().clone()).unwrap_or_default();
    let scan = database.sync_prepare_outbox_excluding_positions(positions.waiting(web_time::Instant::now())).await?;
    Ok(PublicationOutbox { scan, positions })
}

pub(crate) async fn next_outbox_page(database: &DatabaseClient, events: Option<&LibraryEventRouter>, outbox: &mut PublicationOutbox) -> Result<Vec<StatePublication>, DatabaseError> {
    let page = database.sync_next_outbox_page(&mut outbox.scan).await?;
    if let Some(events) = events {
        let selected = outbox.positions.revisions(page.iter().filter_map(|publication| match &publication.state.value {
            LibraryStateEntry::ReadingPosition { book_id, .. } => Some(*book_id),
            _ => None,
        }));
        if !selected.is_empty() {
            events.emit(LibraryEvent::ReadingPositionsSelected(selected));
        }
    }
    Ok(page)
}

/// Storage commands for a complete native exchange. The local
/// library is authoritative, so commands never validate a stored account
/// binding: switching accounts leaves sync state untouched.
pub(super) struct AccountStore<'a> {
    pub sync: &'a super::LibrarySync,
}

pub(crate) async fn acknowledge(database: &DatabaseClient, events: Option<&LibraryEventRouter>, ids: &[ReplicaSeq]) -> Result<(), DatabaseError> {
    for chunk in ids.chunks(MAX_PUSH_MUTATIONS) {
        database.sync_acknowledge_mutations(chunk).await?;
    }
    if !ids.is_empty() {
        if let Some(events) = events {
            events.assets.wake();
        }
    }
    Ok(())
}

/// One bounded exchange commits before notifying the owner. No transaction
/// spans an await; independently valid acknowledgements survive a failed pull.
pub(crate) async fn apply_response(database: &DatabaseClient, events: Option<&LibraryEventRouter>, ids: &[ReplicaSeq], page: &PullStateResponse) -> Result<(), DatabaseError> {
    // Normal replies fit the existing protocol limit. Retain bounded handling
    // for callers supplying more IDs than a single exchange could acknowledge.
    let ids = if ids.len() > MAX_PUSH_MUTATIONS {
        acknowledge(database, events, ids).await?;
        &[]
    } else {
        ids
    };
    match database.sync_commit_pull_response(ids, page).await {
        Ok(changes) => {
            if let Some(events) = events {
                if !ids.is_empty() {
                    events.assets.wake();
                }
                if changes.contents_changed {
                    events.emit(LibraryEvent::ContentsChanged);
                } else {
                    for book in changes.reading_progress {
                        events.updates.publish(LibraryUpdate::ReadingProgress(book));
                    }
                }
            }
            Ok(())
        }
        Err(error) if !ids.is_empty() => {
            // The combined command rolled back. Commit only its acknowledgements
            // under a fresh transaction, then report the original pull error.
            acknowledge(database, events, ids).await?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

impl crate::engine::SyncStore for AccountStore<'_> {
    type Outbox = PublicationOutbox;

    fn prepare_sync(&self) -> StoreFuture<'_, PreparedSync<Self::Outbox>> {
        Box::pin(async { prepare_sync(&self.sync.database, Some(&self.sync.events)).await.map_err(Into::into) })
    }
    fn prepare_outbox(&self) -> StoreFuture<'_, Self::Outbox> {
        Box::pin(async { prepare_outbox(&self.sync.database, Some(&self.sync.events)).await.map_err(Into::into) })
    }
    fn record_mutation_issues(&self, issues: &[sync_common::MutationIssue]) -> StoreFuture<'_, ()> {
        let issues = issues.to_vec();
        Box::pin(async move {
            self.sync.database.sync_record_mutation_issues(&issues).await?;
            self.sync.events.refresh_contents();
            Ok(())
        })
    }
    fn begin_cursor_recovery(&self) -> StoreFuture<'_, ()> {
        Box::pin(async { self.sync.database.sync_begin_cursor_recovery().await.map_err(Into::into) })
    }
    fn complete_cursor_recovery(&self) -> StoreFuture<'_, usize> {
        Box::pin(async { crate::recovery::complete(&self.sync.database).await.map_err(Into::into) })
    }
    fn pending_mutations<'a>(&'a self, scan: &'a mut Self::Outbox) -> StoreFuture<'a, Vec<library_replica::StatePublication>> {
        Box::pin(async move { next_outbox_page(&self.sync.database, Some(&self.sync.events), scan).await.map_err(Into::into) })
    }
    fn acknowledge_mutations(&self, ids: &[ReplicaSeq]) -> StoreFuture<'_, ()> {
        let ids = ids.to_vec();
        Box::pin(async move { acknowledge(&self.sync.database, Some(&self.sync.events), &ids).await.map_err(Into::into) })
    }
    fn apply_exchange_response(&self, ids: Vec<ReplicaSeq>, page: PullStateResponse) -> StoreFuture<'_, ()> {
        Box::pin(async move { apply_response(&self.sync.database, Some(&self.sync.events), &ids, &page).await.map_err(Into::into) })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use book_model::{ChapterReadStatus, ReadingPosition};
    use client_platform_runtime::executor::BackendExecutor;
    use library_replica::{BookLifecycleState, ReadingPositionState};
    use std::cell::Cell;
    use sync_common::{BookId, Delivery, DirId, LibraryRevision, ReplicaId, SyncCursor, ROOT_DIR_ID};

    #[tokio::test]
    async fn pull_notifications_classify_accepted_changes_and_ignore_stale_echoes() {
        let directory = tempfile::tempdir().unwrap();
        let database = DatabaseClient::open(directory.path().join("pull-notifications.sqlite3"), &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        let first = BookId::from_bytes([1; 32]);
        let second = BookId::from_bytes([2; 32]);
        let sequence = Cell::new(0);
        let publication = |entry: LibraryStateEntry, time| {
            sequence.set(sequence.get() + 1);
            entry.publication(ReplicaId::from_u128(8), time, ReplicaSeq::new(sequence.get()).unwrap())
        };
        let page = |publications: &[StatePublication]| PullStateResponse {
            mutations: publications.iter().enumerate().map(|(index, publication)| Delivery { state: sync_common::transport::representation(&publication.state).unwrap(), revision: LibraryRevision::new(index as u64 + 1).unwrap() }).collect(),
            next_cursor: SyncCursor::default(),
            has_more: false,
        };
        let lifecycles = [first, second].map(|book_id| publication(LibraryStateEntry::BookLifecycle { book_id, value: BookLifecycleState::Present }, 1));
        apply_response(&database, None, &[], &page(&lifecycles)).await.unwrap();

        let (events, mut state) = LibraryEventRouter::new();
        let updates = state.updates.subscribe();
        let position = |book_id, time| publication(LibraryStateEntry::ReadingPosition { book_id, value: ReadingPositionState { location: ReadingPosition::pdf_page(3), progress: 25.0 } }, time);
        let progress = page(&[position(first, 10), publication(LibraryStateEntry::ChapterReadStatus { book_id: first, value: ChapterReadStatus::default() }, 10), position(second, 10)]);
        apply_response(&database, Some(&events), &[], &progress).await.unwrap();
        state.drain(&events);
        assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::ReadingProgress(first));
        assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::ReadingProgress(second));
        assert!(updates.try_recv().is_err(), "position and chapter status share one notification per book");

        apply_response(&database, Some(&events), &[], &progress).await.unwrap();
        apply_response(&database, Some(&events), &[], &page(&[position(first, 9)])).await.unwrap();
        apply_response(&database, Some(&events), &[], &page(&[])).await.unwrap();
        state.drain(&events);
        assert!(updates.try_recv().is_err(), "repeated, stale, and empty pages must not refresh readers");

        let mixed = page(&[position(first, 11), publication(LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(9), name: "Remote shelf".into() }, 11)]);
        apply_response(&database, Some(&events), &[], &mixed).await.unwrap();
        state.drain(&events);
        assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::Contents);
        assert!(updates.try_recv().is_err(), "a contents notification already includes progress changes");

        apply_response(&database, Some(&events), &[], &mixed).await.unwrap();
        let stale_contents = page(&[publication(LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(9), name: "Old shelf name".into() }, 9)]);
        apply_response(&database, Some(&events), &[], &stale_contents).await.unwrap();
        state.drain(&events);
        assert!(updates.try_recv().is_err(), "stale structural mutations must not reload annotations either");
    }

    #[tokio::test]
    async fn pending_recovery_does_not_freeze_or_select_the_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recover.sqlite3");
        let database = DatabaseClient::open(&path, &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        database.create_directory_with_id(&DirId::from_u128(1), &ROOT_DIR_ID, &"Shelf".to_owned()).await.unwrap();
        database.sync_begin_cursor_recovery().await.unwrap();
        let probe = rusqlite::Connection::open(&path).unwrap();
        // Normal preparation freezes pending rows with UPDATE before reading them.
        probe.execute_batch("CREATE TRIGGER reject_freeze_before_recovery BEFORE UPDATE ON sync_outbox BEGIN SELECT RAISE(ABORT, 'recovery must finish before outbox preparation'); END;").unwrap();
        assert!(matches!(prepare_sync(&database, None).await.unwrap(), PreparedSync::RecoveryPending));
        assert!(database.sync_cursor_recovery_pending().await.unwrap());
    }

    #[tokio::test]
    async fn an_empty_preparation_roundtrips_through_the_worker_codec() {
        let directory = tempfile::tempdir().unwrap();
        let database = DatabaseClient::open(directory.path().join("empty.sqlite3"), &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        let PreparedSync::Ready { cursor, mutations, .. } = prepare_sync(&database, None).await.unwrap() else {
            panic!("no recovery is pending");
        };
        assert!(mutations.is_empty());
        let bytes = rmp_serde::to_vec_named(&PreparedSync::Ready { cursor, outbox: (), mutations }).unwrap();
        let prepared: PreparedSync<()> = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(prepared, PreparedSync::Ready { cursor: decoded, mutations, .. } if decoded == cursor && mutations.is_empty()));
    }
}
