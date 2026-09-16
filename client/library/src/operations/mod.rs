//! Durable library workflows above file, thumbnail, and synchronization jobs.
//! Records are device-local. Progress comes from durable work, not job lifetimes.
use library_replica::{LibraryOperation, LibraryOperationKind as Kind, LibraryOperationState as State};
use rusqlite::{Connection, OptionalExtension};
#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    id: String,
    kind: Kind,
    scan_complete: bool,
    scan_failed: bool,
    discovered: u64,
    failures: u64,
    requires_sync: bool,
    completed: bool,
    #[serde(default)]
    sync_unfinished: bool,
}
impl Record {
    fn new(kind: Kind, requires_sync: bool) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), kind, scan_complete: kind == Kind::Sync, scan_failed: false, discovered: 0, failures: 0, requires_sync, completed: false, sync_unfinished: false }
    }
}
fn read(conn: &Connection) -> rusqlite::Result<Option<Record>> {
    let value: Option<String> = conn.prepare_cached(include_str!("sql/read.sql"))?.query_row([], |row| row.get(0)).optional()?;
    value.map(|value| serde_json::from_str(&value).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))).transpose()
}
fn write(conn: &Connection, record: &Record) -> rusqlite::Result<()> {
    let value = serde_json::to_string(record).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.prepare_cached(include_str!("sql/write.sql"))?.execute([value])?;
    Ok(())
}

pub fn begin_import(conn: &Connection, requires_sync: bool) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let mut record = read(&tx)?.filter(|record| !record.completed && record.kind == Kind::Import).unwrap_or_else(|| Record::new(Kind::Import, requires_sync));
    record.scan_complete = false;
    record.scan_failed = false;
    record.requires_sync |= requires_sync;
    write(&tx, &record)?;
    tx.commit()
}

/// Preserve an existing import as the owner of its metadata sync jobs.
pub fn begin_sync(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let current = read(&tx)?;
    if current.as_ref().is_some_and(|record| !record.completed && record.requires_sync && record.sync_unfinished == (record.kind == Kind::Sync)) {
        tx.commit()?;
        return Ok(());
    }
    let mut record = current.filter(|record| !record.completed).unwrap_or_else(|| Record::new(Kind::Sync, true));
    record.requires_sync = true;
    // Import already has durable scan and upload/outbox dependencies. Do not
    // add another pair of disk commits for every metadata batch it produces.
    record.sync_unfinished = record.kind == Kind::Sync;
    write(&tx, &record)?;
    tx.commit()
}

pub fn needs_sync(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(read(conn)?.is_some_and(|record| !record.completed && record.sync_unfinished))
}

pub fn finish_sync(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    finish_sync_in_transaction(&tx)?;
    tx.commit()
}

/// Join the metadata checkpoint's commit instead of syncing the disk twice.
pub fn finish_sync_in_transaction(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    if let Some(mut record) = read(tx)?.filter(|record| !record.completed && record.sync_unfinished) {
        record.sync_unfinished = false;
        write(tx, &record)?;
    }
    Ok(())
}

/// An interrupted scan is resumed when its library handle is reopened.
pub fn needs_scan(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(read(conn)?.is_some_and(|record| !record.completed && record.kind == Kind::Import && !record.scan_complete))
}

pub fn finish_scan(conn: &Connection, discovered: u64, failures: u64, successful: bool) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    if let Some(mut record) = read(&tx)?.filter(|record| !record.completed && record.kind == Kind::Import) {
        record.scan_complete = successful;
        record.scan_failed = !successful;
        record.discovered = record.discovered.max(discovered);
        record.failures = record.failures.max(failures);
        write(&tx, &record)?;
    }
    tx.commit()
}

#[derive(Default)]
pub struct Activity<'a> {
    pub asset_uploads_disabled: bool,
    pub scanning: bool,
    pub discovered: u64,
    pub scan_failures: u64,
    pub local_busy: bool,
    pub local_waiting: bool,
    pub sync_busy: bool,
    pub signed_in: bool,
    pub file_error: Option<&'a str>,
    pub transfer_retrying: bool,
}

/// Reconcile a workflow against a consistent queue snapshot. Only workflow
/// creation/completion writes a record; ordinary progress reads do not commit.
pub struct OperationStatus {
    pub operation: Option<LibraryOperation>,
    pub total_books: u64,
    pub uploaded_books: u64,
}

pub fn status(conn: &Connection, activity: Activity<'_>) -> rusqlite::Result<OperationStatus> {
    let tx = conn.unchecked_transaction()?;
    let (books, uploaded_books, pending_uploads, pending_changes, pending_thumbnails, pending_covers, rejections, checkpoint): (u64, u64, u64, u64, u64, u64, u64, bool) =
        tx.prepare_cached(include_str!("sql/progress.sql"))?.query_row([], |r| {
            Ok((
                r.get::<_, i64>(0)?.max(0) as u64,
                r.get::<_, i64>(1)?.max(0) as u64,
                r.get::<_, i64>(2)?.max(0) as u64,
                r.get::<_, i64>(3)?.max(0) as u64,
                r.get::<_, i64>(4)?.max(0) as u64,
                r.get::<_, i64>(5)?.max(0) as u64,
                r.get::<_, i64>(6)?.max(0) as u64,
                r.get(7)?,
            ))
        })?;
    let (pending_uploads, pending_covers, rejections) = if activity.asset_uploads_disabled { (0, 0, 0) } else { (pending_uploads, pending_covers, rejections) };
    let mut stored = read(&tx)?;
    let remote_pending = pending_uploads + pending_changes + pending_covers > 0;
    if stored.as_ref().is_none_or(|record| record.completed) {
        if activity.scanning {
            let record = Record::new(Kind::Import, activity.signed_in);
            write(&tx, &record)?;
            stored = Some(record);
        } else if activity.signed_in && (remote_pending || activity.sync_busy) {
            let record = Record::new(Kind::Sync, true);
            write(&tx, &record)?;
            stored = Some(record);
        } else {
            tx.commit()?;
            return Ok(OperationStatus { operation: None, total_books: books, uploaded_books });
        }
    }
    let mut record = stored.expect("operation established above");
    if activity.signed_in && !record.requires_sync {
        record.requires_sync = true;
        write(&tx, &record)?;
    }
    let local_pending = !record.scan_complete || activity.scanning || activity.local_busy || pending_thumbnails > 0;
    let sync_pending = record.requires_sync && (remote_pending || activity.sync_busy || record.sync_unfinished || !checkpoint);
    let waiting_reason = if record.scan_failed {
        Some("Scan interrupted; reopen the library to retry".to_owned())
    } else if let Some(error) = activity.file_error {
        Some(error.to_owned())
    } else if sync_pending && !activity.signed_in {
        Some("Sign in to finish syncing".to_owned())
    } else if record.requires_sync && (rejections > 0 || activity.transfer_retrying) {
        Some("Waiting to retry uploads".to_owned())
    } else if !activity.scanning && activity.local_waiting && pending_thumbnails > 0 {
        Some("Waiting to retry thumbnails".to_owned())
    } else if !activity.scanning && !activity.local_busy && !activity.sync_busy && (local_pending || sync_pending) {
        Some("Waiting for background work".to_owned())
    } else {
        None
    };
    let complete = !local_pending && !sync_pending && waiting_reason.is_none();
    if complete {
        record.completed = true;
        write(&tx, &record)?;
    }
    let result = LibraryOperation {
        id: record.id,
        kind: record.kind,
        state: if complete {
            State::Completed
        } else if waiting_reason.is_some() {
            State::Waiting
        } else {
            State::Running
        },
        scanning: activity.scanning,
        // Counts describe library state and never reset for an individual batch.
        books: books.max(record.discovered).max(activity.discovered),
        uploaded_books,
        pending_uploads,
        pending_changes,
        pending_thumbnails,
        failures: record.failures.max(activity.scan_failures) + rejections,
        needs_attention: record.scan_failed
            || activity.file_error.is_some()
            || (sync_pending && !activity.signed_in)
            || (record.requires_sync && (rejections > 0 || activity.transfer_retrying))
            || (!activity.scanning && activity.local_waiting && pending_thumbnails > 0),
        waiting_reason,
    };
    tx.commit()?;
    Ok(OperationStatus { operation: Some(result), total_books: books, uploaded_books })
}
