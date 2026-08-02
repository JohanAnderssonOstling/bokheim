//! Deterministic synchronization-protocol convergence harness.
//!
//! These tests exercise the real SQLite schema, triggers, outbox, and remote
//! reducer against an in-process model of the server. Every discovered
//! correctness counterexample remains enabled as a regression test.
//!
//! Run the controls:
//! `cargo test -p epub-svelte convergence_tests`
//!
//! Exercise the real HTTP/PostgreSQL boundary as well:
//! `Backend/tests/run_sync_e2e.sh`

use crate::{db, BookId, ContentHash};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use sync_common::{ActorId, DirId, DirectoryState, EventId, FileName, Hlc, LocalChange, LocalSeq, ServerChange, ServerSeq, SyncOp};

#[derive(Clone)]
struct StoredEvent {
    change: ServerChange,
}

#[derive(Default)]
struct FakeServer {
    next_server_seq: u64,
    append: Vec<StoredEvent>,
    append_ids: HashSet<EventId>,
    singletons: HashMap<(String, String), StoredEvent>,
}

impl FakeServer {
    fn push(&mut self, actor_id: &ActorId, changes: Vec<LocalChange>) {
        for change in changes {
            self.accept(actor_id, change);
        }
    }

    fn accept(&mut self, actor_id: &ActorId, change: LocalChange) {
        if !change.op.is_semantically_valid() {
            return;
        }
        // Match the production server: request identity, not client JSON,
        // determines the authoritative actor and actor-local sequence.
        let candidate = ServerChange {
            change,
            actor_id: actor_id.clone(),
            // `server_seq` is not part of VersionKey, so a provisional next
            // value is safe while deciding whether a singleton is accepted.
            server_seq: ServerSeq::new(self.next_server_seq + 1).expect("positive server sequence"),
        };
        if candidate.op.is_singleton() {
            let key = event_key(&candidate);
            let incoming_wins = self.singletons.get(&key).is_none_or(|stored| candidate.version_key() > stored.change.version_key());
            if incoming_wins {
                self.next_server_seq += 1;
                self.singletons.insert(key, StoredEvent { change: candidate });
            }
            return;
        }

        let id = candidate.event_id;
        if !self.append_ids.insert(id) {
            return;
        }
        self.next_server_seq += 1;
        self.append.push(StoredEvent { change: candidate });
    }

    fn fetch(&self, _actor_id: &ActorId, cursor: u64) -> Vec<ServerChange> {
        let mut rows: Vec<&StoredEvent> = self.append.iter().chain(self.singletons.values()).collect();
        rows.sort_by_key(|row| row.change.server_seq.get());
        rows.into_iter().filter(|row| row.change.server_seq.get() > cursor).map(|row| row.change.clone()).collect()
    }

    fn all_rows(&self) -> Vec<&StoredEvent> {
        self.append.iter().chain(self.singletons.values()).collect()
    }

    /// Mirrors blob GC: resolve every placement with the protocol's total
    /// VersionKey, independently of server arrival order.
    fn live_blob_hashes_by_version(&self) -> HashSet<ContentHash> {
        let mut latest_by_occupant: HashMap<(BookId, DirId, FileName), &StoredEvent> = HashMap::new();
        for row in self.all_rows() {
            let Some(key) = placement_key(&row.change.op) else {
                continue;
            };
            let replace = latest_by_occupant.get(&key).is_none_or(|stored| row.change.version_key() > stored.change.version_key());
            if replace {
                latest_by_occupant.insert(key, row);
            }
        }

        let mut latest_by_path: HashMap<(DirId, FileName), &StoredEvent> = HashMap::new();
        for row in latest_by_occupant.values() {
            let SyncOp::BookDirAdded { dir_id, file_name, .. } = &row.change.op else { continue };
            let key = (dir_id.clone(), file_name.clone());
            if latest_by_path.get(&key).is_none_or(|stored| row.change.version_key() > stored.change.version_key()) {
                latest_by_path.insert(key, row);
            }
        }
        latest_by_path
            .values()
            .filter_map(|row| match &row.change.op {
                SyncOp::BookDirAdded { content_hash, .. } => Some(content_hash.clone()),
                _ => None,
            })
            .collect()
    }
}

struct Device {
    actor_id: ActorId,
    cursor: u64,
    conn: Connection,
}

impl Device {
    fn new(actor_id: &str) -> Self {
        let conn = Connection::open_in_memory().expect("open device database");
        db::init_db(&conn).unwrap();
        // Production passes the client id persisted in this same database to
        // every push. Keep the modeled request identity and trigger-stamped
        // local identity identical as well.
        db::set_sync_metadata(&conn, "client_id", actor_id).unwrap();
        Self { actor_id: ActorId::parse(actor_id).expect("valid test actor id"), cursor: 0, conn }
    }

    fn seed_book(&self, book_id: BookId) {
        self.remote_sql(
            "INSERT OR IGNORE INTO book(id, title, added_at, deleted_at)
             VALUES (?1, 'seed', 1, NULL)",
            params![book_id.canonical()],
        );
    }

    fn seed_dir(&self, dir_id: &str, name: &str) {
        self.remote_sql(
            "INSERT OR IGNORE INTO dir(id, parent_id, name, deleted_at)
             VALUES (?1, '__root__', ?2, NULL)",
            params![dir_id, name],
        );
    }

    fn seed_placement(&self, book_id: BookId, dir_id: &str, file_name: &str, content_hash: &ContentHash, hlc: Hlc) {
        let event_id = "11111111-1111-5111-8111-111111111111";
        self.remote_sql(
            "INSERT INTO book_dir(
                 dir_id, book_id, file_name, local_hash, content_hash,
                 is_downloaded, last_scan, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, intent_deleted, deleted_at
             ) VALUES (?1, ?2, ?3, 'fingerprint', ?4, 1, 1, ?5, ?6, '0-legacy', 1, ?7, 0, NULL)
             ON CONFLICT(dir_id, book_id, file_name) DO UPDATE SET
                 content_hash=excluded.content_hash,
                 hlc_pt=excluded.hlc_pt,
                 hlc_l=excluded.hlc_l,
                 hlc_actor=excluded.hlc_actor,
                 hlc_seq=excluded.hlc_seq,
                 hlc_event=excluded.hlc_event,
                 intent_deleted=0,
                 deleted_at=NULL",
            params![dir_id, book_id.canonical(), file_name, content_hash.as_str(), hlc.pt as i64, hlc.l as i64, event_id],
        );
    }

    fn set_reading_position(&self, book_id: BookId, cfi: &str) {
        self.remote_sql("UPDATE book SET read_pos=?2 WHERE id=?1", params![book_id.canonical(), cfi]);
    }

    fn set_book_deleted(&self, book_id: BookId, deleted: bool) {
        let deleted_at = deleted.then_some(1_i64);
        self.remote_sql("UPDATE book SET deleted_at=?2 WHERE id=?1", params![book_id.canonical(), deleted_at]);
    }

    fn set_placement_deleted(&self, book_id: BookId, dir_id: &str, file_name: &str, deleted: bool, hlc: Hlc) {
        let deleted_at = deleted.then_some(1_i64);
        self.remote_sql(
            "UPDATE book_dir
             SET deleted_at=?4, hlc_pt=?5, hlc_l=?6
             WHERE dir_id=?1 AND book_id=?2 AND file_name=?3",
            params![dir_id, book_id.canonical(), file_name, deleted_at, hlc.pt as i64, hlc.l as i64],
        );
    }

    fn queue(&self, change: &LocalChange) {
        let entity_id = change.op.entity_id().into_owned();
        self.conn
            .execute(
                "INSERT INTO event_log(entity_id, event_type, payload, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![entity_id, change.op.event_type(), change.op.payload().to_string(), change.changed_at as i64],
            )
            .expect("queue local event");
        let local_seq = self.conn.last_insert_rowid();
        self.conn.execute("UPDATE event_log SET hlc_pt=?2, hlc_l=?3 WHERE local_seq=?1", params![local_seq, change.hlc.pt as i64, change.hlc.l as i64]).expect("force deterministic test HLC");
        if matches!(change.op, SyncOp::BookAdded { .. } | SyncOp::BookDeleted { .. }) {
            self.conn
                .execute(
                    "UPDATE book
                     SET hlc_pt=?2, hlc_l=?3, hlc_actor=?4, hlc_seq=?5
                     WHERE id=?1",
                    params![change.op.entity_id().as_ref(), change.hlc.pt as i64, change.hlc.l as i64, self.actor_id.as_str(), local_seq],
                )
                .expect("force deterministic book version");
        }
        if let SyncOp::DirState { dir_id, .. } = &change.op {
            self.conn
                .execute("UPDATE dir SET hlc_pt=?2, hlc_l=?3, hlc_actor=?4, hlc_seq=?5 WHERE id=?1", params![dir_id.as_str(), change.hlc.pt as i64, change.hlc.l as i64, self.actor_id.as_str(), local_seq])
                .expect("force deterministic directory version");
        }
    }

    fn push(&mut self, server: &mut FakeServer) {
        let changes = db::get_unpushed_events(&self.conn).expect("decode local outbox");
        let max_seq = changes.iter().map(|change| change.local_seq.get()).max();
        server.push(&self.actor_id, changes);
        if let Some(max_seq) = max_seq {
            db::mark_events_pushed(&self.conn, max_seq).unwrap();
        }
    }

    fn pull(&mut self, server: &FakeServer) {
        self.try_pull(server).expect("apply remote change");
    }

    fn try_pull(&mut self, server: &FakeServer) -> Result<(), Box<dyn std::error::Error>> {
        let changes = server.fetch(&self.actor_id, self.cursor);
        if changes.is_empty() {
            return Ok(());
        }

        let next_cursor = changes.last().expect("non-empty changes").server_seq.get();
        let tx = self.conn.transaction()?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        for change in &changes {
            // Intentionally ignore filesystem work returned for removals. Tests
            // can model a crash at the production DB/filesystem durability gap.
            db::apply_remote_change_in_tx(&tx, change)?;
        }
        tx.commit()?;
        self.cursor = next_cursor;
        Ok(())
    }

    fn read_position(&self, book_id: BookId) -> String {
        self.conn.query_row("SELECT read_pos FROM book WHERE id=?1", params![book_id.canonical()], |row| row.get(0)).expect("book read position")
    }

    fn reading_state(&self, book_id: BookId) -> (Option<String>, i64) {
        self.conn.query_row("SELECT read_pos, last_read FROM book WHERE id=?1", params![book_id.canonical()], |row| Ok((row.get(0)?, row.get(1)?))).expect("book reading state")
    }

    fn book_is_live(&self, book_id: BookId) -> bool {
        self.conn.query_row("SELECT deleted_at IS NULL FROM book WHERE id=?1", params![book_id.canonical()], |row| row.get(0)).expect("book liveness")
    }

    fn placement_hash(&self, book_id: BookId, dir_id: &str, file_name: &str) -> Option<ContentHash> {
        self.conn
            .query_row(
                "SELECT content_hash FROM book_dir
                 WHERE dir_id=?1 AND book_id=?2 AND file_name=?3
                   AND deleted_at IS NULL",
                params![dir_id, book_id.canonical(), file_name],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|hash| ContentHash::parse(&hash).ok())
    }

    fn remote_sql<P>(&self, sql: &str, params: P)
    where
        P: rusqlite::Params,
    {
        db::set_sync_metadata(&self.conn, "change_origin", "remote").unwrap();
        self.conn.execute(sql, params).expect("seed remote state");
        db::set_sync_metadata(&self.conn, "change_origin", "local").unwrap();
    }
}

fn event_key(change: &ServerChange) -> (String, String) {
    (change.op.entity_id().into_owned(), change.op.event_type().to_string())
}

fn placement_key(op: &SyncOp) -> Option<(BookId, DirId, FileName)> {
    match op {
        SyncOp::BookDirAdded { book_id, dir_id, file_name, .. } | SyncOp::BookDirRemoved { book_id, dir_id, file_name } => Some((*book_id, dir_id.clone(), file_name.clone())),
        _ => None,
    }
}

fn change(op: SyncOp, pt: u64) -> LocalChange {
    LocalChange { event_id: EventId::new(), op, changed_at: pt, hlc: Hlc { pt, l: 0 }, local_seq: LocalSeq::new(1).unwrap() }
}

fn placement_add(book_id: BookId, dir_id: &str, file_name: &str, content_hash: &ContentHash, pt: u64) -> LocalChange {
    change(SyncOp::BookDirAdded { book_id, dir_id: DirId::parse(dir_id).unwrap(), file_name: FileName::parse(file_name).unwrap(), content_hash: content_hash.clone() }, pt)
}

fn placement_remove(book_id: BookId, dir_id: &str, file_name: &str, pt: u64) -> LocalChange {
    change(SyncOp::BookDirRemoved { book_id, dir_id: DirId::parse(dir_id).unwrap(), file_name: FileName::parse(file_name).unwrap() }, pt)
}

#[test]
fn fake_server_deduplicates_append_retries() {
    let mut server = FakeServer::default();
    let event = change(SyncOp::BookAdded { book_id: BookId::new(1), title: "Book".to_string(), added_at: 1 }, 1);

    let actor_id = ActorId::parse("device").unwrap();
    server.accept(&actor_id, event.clone());
    server.accept(&actor_id, event);

    assert_eq!(server.append.len(), 1);
}

#[test]
fn higher_hlc_placement_add_converges() {
    let book_id = BookId::new(1);
    let old_hash = ContentHash::parse(&"a".repeat(64)).unwrap();
    let new_hash = ContentHash::parse(&"b".repeat(64)).unwrap();
    let mut server = FakeServer::default();
    let mut a = Device::new("a");
    let mut b = Device::new("b");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(book_id);
    }

    a.seed_placement(book_id, "d1", "book.epub", &old_hash, Hlc { pt: 10, l: 0 });
    a.queue(&placement_add(book_id, "d1", "book.epub", &old_hash, 10));
    // queue() drives the real event-log HLC trigger, which also stamps the
    // placement. Restore the deterministic clock selected by this scenario.
    a.seed_placement(book_id, "d1", "book.epub", &old_hash, Hlc { pt: 10, l: 0 });
    b.seed_placement(book_id, "d1", "book.epub", &new_hash, Hlc { pt: 20, l: 0 });
    b.queue(&placement_add(book_id, "d1", "book.epub", &new_hash, 20));
    b.seed_placement(book_id, "d1", "book.epub", &new_hash, Hlc { pt: 20, l: 0 });

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.placement_hash(book_id, "d1", "book.epub"), Some(new_hash.clone()));
    assert_eq!(b.placement_hash(book_id, "d1", "book.epub"), Some(new_hash));
}

#[test]
fn equal_hlc_singleton_conflict_converges() {
    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    // z wins the server's exact-HLC actor-id tie.
    let mut a = Device::new("z-device");
    let mut b = Device::new("a-device");
    for device in [&a, &b] {
        device.seed_book(book_id);
    }

    let a_change = change(SyncOp::ReadingPositionUpdated { book_id, cfi: "epubcfi(/6/2)".to_string() }, 100);
    a.set_reading_position(book_id, "epubcfi(/6/2)");
    a.queue(&a_change);

    let b_change = change(SyncOp::ReadingPositionUpdated { book_id, cfi: "epubcfi(/6/4)".to_string() }, 100);
    b.set_reading_position(book_id, "epubcfi(/6/4)");
    b.queue(&b_change);

    b.push(&mut server);
    a.pull(&server);
    a.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.read_position(book_id), b.read_position(book_id));
}

#[test]
fn repeated_partition_heal_cycles_converge_in_both_reconnect_orders() {
    const CYCLES: usize = 24;
    const OFFLINE_UPDATES_PER_CYCLE: usize = 7;

    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    let mut devices = vec![Device::new("partition-a"), Device::new("partition-b"), Device::new("partition-c")];
    let device_count = devices.len();
    for device in &devices {
        device.seed_book(book_id);
    }

    for cycle in 0..CYCLES {
        let isolated = cycle % device_count;
        let base_hlc = 1_000 + cycle as u64 * 100;
        let offline_wins = cycle % 2 == 1;

        // The isolated replica accumulates several local singleton updates.
        // Odd cycles give it the greatest version; even cycles deliberately
        // make its entire offline history stale.
        for update in 0..OFFLINE_UPDATES_PER_CYCLE {
            let pt = base_hlc + update as u64 + if offline_wins { 50 } else { 0 };
            let cfi = format!("epubcfi(/6/{})", cycle * 100 + update + 2);
            devices[isolated].set_reading_position(book_id, &cfi);
            devices[isolated].queue(&change(SyncOp::ReadingPositionUpdated { book_id, cfi }, pt));
        }

        // Connected replicas continue independently while the selected device
        // is partitioned.
        for offset in 1..device_count {
            let actor = (isolated + offset) % device_count;
            let pt = base_hlc + 20 + offset as u64;
            let cfi = format!("epubcfi(/8/{})", cycle * 10 + offset);
            devices[actor].set_reading_position(book_id, &cfi);
            devices[actor].queue(&change(SyncOp::ReadingPositionUpdated { book_id, cfi }, pt));
            devices[actor].push(&mut server);
        }
        for offset in 1..device_count {
            let actor = (isolated + offset) % device_count;
            devices[actor].pull(&server);
        }

        // Exercise both important reconnect orders. Pull-first must protect a
        // newer unpushed local register; push-first must still accept a newer
        // remote winner afterward.
        if cycle % 2 == 0 {
            devices[isolated].pull(&server);
            devices[isolated].push(&mut server);
        } else {
            devices[isolated].push(&mut server);
            devices[isolated].pull(&server);
        }
        for device in &mut devices {
            device.pull(&server);
        }

        let expected = devices[0].read_position(book_id);
        for (index, device) in devices.iter().enumerate().skip(1) {
            assert_eq!(device.read_position(book_id), expected, "cycle {cycle}: replica {index} diverged after partition healing");
        }
    }

    for device in &devices {
        assert!(db::get_unpushed_events(&device.conn).unwrap().is_empty(), "healed replica retained acknowledged outbox events");
    }
    assert_eq!(server.singletons.keys().filter(|(_, event_type)| event_type == "reading_position_updated").count(), 1, "repeated offline histories must collapse to one server register");
}

#[test]
fn concurrent_book_add_delete_converges() {
    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_book(book_id);
    }

    a.queue(&change(SyncOp::BookAdded { book_id, title: "seed".to_string(), added_at: 1 }, 100));
    b.set_book_deleted(book_id, true);
    b.queue(&change(SyncOp::BookDeleted { book_id }, 101));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.book_is_live(book_id), b.book_is_live(book_id));
    assert!(!a.book_is_live(book_id), "the higher-version deletion must win");
}

#[test]
fn equal_hlc_book_add_delete_converges_to_deleted() {
    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    let mut a = Device::new("z-device");
    let mut b = Device::new("a-device");
    for device in [&a, &b] {
        device.seed_book(book_id);
    }

    a.queue(&change(SyncOp::BookAdded { book_id, title: "seed".to_string(), added_at: 1 }, 100));
    b.set_book_deleted(book_id, true);
    b.queue(&change(SyncOp::BookDeleted { book_id }, 100));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert!(!a.book_is_live(book_id));
    assert!(!b.book_is_live(book_id));
}

#[test]
fn equal_hlc_placement_adds_converge() {
    let book_id = BookId::new(1);
    let a_hash = ContentHash::parse(&"a".repeat(64)).unwrap();
    let b_hash = ContentHash::parse(&"b".repeat(64)).unwrap();
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(book_id);
    }

    a.seed_placement(book_id, "d1", "book.epub", &a_hash, Hlc { pt: 100, l: 0 });
    a.queue(&placement_add(book_id, "d1", "book.epub", &a_hash, 100));
    a.seed_placement(book_id, "d1", "book.epub", &a_hash, Hlc { pt: 100, l: 0 });
    b.seed_placement(book_id, "d1", "book.epub", &b_hash, Hlc { pt: 100, l: 0 });
    b.queue(&placement_add(book_id, "d1", "book.epub", &b_hash, 100));
    b.seed_placement(book_id, "d1", "book.epub", &b_hash, Hlc { pt: 100, l: 0 });

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.placement_hash(book_id, "d1", "book.epub"), b.placement_hash(book_id, "d1", "book.epub"));
}

#[test]
fn crash_after_pull_commit_does_not_resurrect_removed_file() {
    let book_id = BookId::new(1);
    let content_hash = ContentHash::parse(&"c".repeat(64)).unwrap();
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(book_id);
        device.seed_placement(book_id, "d1", "book.epub", &content_hash, Hlc { pt: 10, l: 0 });
    }
    let disk = tempfile::tempdir().expect("temporary library");
    let shelf = disk.path().join("Shelf");
    std::fs::create_dir_all(&shelf).expect("create shelf");
    std::fs::write(shelf.join(".biblos_uuid"), "d1").expect("seed shelf identity");
    let book_path = shelf.join("book.epub");
    std::fs::write(&book_path, b"still present at crash time").expect("seed local file");

    server.accept(&ActorId::parse("seed-device").unwrap(), placement_add(book_id, "d1", "book.epub", &content_hash, 10));
    b.set_placement_deleted(book_id, "d1", "book.epub", true, Hlc { pt: 20, l: 0 });
    b.queue(&placement_remove(book_id, "d1", "book.epub", 20));
    b.set_placement_deleted(book_id, "d1", "book.epub", true, Hlc { pt: 20, l: 0 });
    b.push(&mut server);

    // Device::pull deliberately stops at the production durability boundary.
    // The cursor and filesystem outbox commit, but no filesystem work runs,
    // modeling a process crash immediately after tx.commit().
    a.pull(&server);

    assert!(a.cursor > 0, "the remote cursor was committed");
    assert!(book_path.exists(), "the modeled crash happens before materialization");
    assert_eq!(db::pending_fs_operation_count(&a.conn).unwrap(), 1, "the removal must be durable beside the cursor");

    // Startup/scanner recovery drains the outbox before walking the library.
    db::drain_pending_fs_operations(&a.conn, disk.path()).unwrap();
    assert!(!book_path.exists());
    assert_eq!(db::pending_fs_operation_count(&a.conn).unwrap(), 0);
}

#[test]
fn stale_removal_cannot_collect_winning_add_blob() {
    let book_id = BookId::new(1);
    let content_hash = ContentHash::parse(&"d".repeat(64)).unwrap();
    let mut server = FakeServer::default();

    server.accept(&ActorId::parse("newer-device").unwrap(), placement_add(book_id, "d1", "book.epub", &content_hash, 200));
    server.accept(&ActorId::parse("stale-device").unwrap(), placement_remove(book_id, "d1", "book.epub", 100));

    assert!(server.live_blob_hashes_by_version().contains(&content_hash), "GC must retain the blob referenced by the VersionKey-winning placement");
}

#[test]
fn removal_of_a_displaced_book_cannot_collect_the_replacement_blob() {
    let old_book = BookId::new(1);
    let new_book = BookId::new(2);
    let old_hash = ContentHash::parse(&"a".repeat(64)).unwrap();
    let replacement_hash = ContentHash::parse(&"b".repeat(64)).unwrap();
    let mut server = FakeServer::default();

    server.accept(&ActorId::parse("old-adder").unwrap(), placement_add(old_book, "d1", "book.epub", &old_hash, 10));
    server.accept(&ActorId::parse("replacer").unwrap(), placement_add(new_book, "d1", "book.epub", &replacement_hash, 20));
    server.accept(&ActorId::parse("late-old-remover").unwrap(), placement_remove(old_book, "d1", "book.epub", 30));

    assert_eq!(server.live_blob_hashes_by_version(), HashSet::from([replacement_hash]));
}

#[test]
fn directory_cycle_resolution_converges_on_the_origin_replicas() {
    let mut server = FakeServer::default();
    let mut a = Device::new("move-a");
    let mut b = Device::new("move-b");
    for device in [&a, &b] {
        device.seed_dir("a", "A");
        device.seed_dir("b", "B");
    }

    let move_a = change(SyncOp::DirState { dir_id: DirId::parse("a").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("b").unwrap(), name: FileName::parse("A").unwrap() } }, 10);
    let move_b = change(SyncOp::DirState { dir_id: DirId::parse("b").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("a").unwrap(), name: FileName::parse("B").unwrap() } }, 11);
    a.remote_sql("UPDATE dir SET parent_id='b' WHERE id='a'", []);
    a.queue(&move_a);
    b.remote_sql("UPDATE dir SET parent_id='a' WHERE id='b'", []);
    b.queue(&move_b);

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, parent_id, deleted_at IS NOT NULL FROM dir WHERE id IN ('a','b') ORDER BY id").unwrap();
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&a), state(&b), "cycle arbitration must not depend on which move was already materialized locally");
    assert!(!state(&a).iter().all(|(_, parent, deleted)| !deleted && (parent == "a" || parent == "b")));
}

#[test]
fn breaking_a_directory_cycle_revives_the_suppressed_directory() {
    let mut server = FakeServer::default();
    let mut existing = Device::new("existing-observer");
    existing.seed_dir("a", "A");
    existing.seed_dir("b", "B");

    server.accept(&ActorId::parse("move-a").unwrap(), change(SyncOp::DirState { dir_id: DirId::parse("a").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("b").unwrap(), name: FileName::parse("A").unwrap() } }, 10));
    server.accept(&ActorId::parse("move-b").unwrap(), change(SyncOp::DirState { dir_id: DirId::parse("b").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("a").unwrap(), name: FileName::parse("B").unwrap() } }, 11));
    existing.pull(&server);

    // Moving A to root breaks the cycle. B's raw register is still the live
    // value `B -> A`, but B receives no new event of its own.
    server.accept(
        &ActorId::parse("cycle-breaker").unwrap(),
        change(SyncOp::DirState { dir_id: DirId::parse("a").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("A").unwrap() } }, 20),
    );
    existing.pull(&server);

    // A fresh replica sees only the server's current singleton values. It must
    // derive the same visible tree as a replica that witnessed the old cycle.
    let mut fresh = Device::new("fresh-observer");
    fresh.pull(&server);
    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, parent_id, deleted_at IS NOT NULL FROM dir WHERE id IN ('a','b') ORDER BY id").unwrap();
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&existing), state(&fresh), "derived cycle suppression must be reversible when the cycle is broken");
}

#[test]
fn same_name_directory_resolution_converges_on_the_origin_replicas() {
    let mut server = FakeServer::default();
    let mut a = Device::new("creator-a");
    let mut b = Device::new("creator-b");
    let first = change(SyncOp::DirState { dir_id: DirId::parse("dir-a").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Shared").unwrap() } }, 10);
    let second = change(SyncOp::DirState { dir_id: DirId::parse("dir-b").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Shared").unwrap() } }, 11);
    a.remote_sql("INSERT INTO dir(id,parent_id,name) VALUES ('dir-a','__root__','Shared')", []);
    a.queue(&first);
    b.remote_sql("INSERT INTO dir(id,parent_id,name) VALUES ('dir-b','__root__','Shared')", []);
    b.queue(&second);

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    let owner = |device: &Device| -> String { device.conn.query_row("SELECT id FROM dir WHERE parent_id='__root__' AND name='Shared' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap() };
    assert_eq!(owner(&a), owner(&b));
}

#[test]
fn renaming_a_same_name_winner_revives_the_suppressed_directory() {
    let mut server = FakeServer::default();
    let mut existing = Device::new("existing-observer");
    for (actor, dir_id, pt) in [("creator-a", "dir-a", 10), ("creator-b", "dir-b", 11)] {
        server.accept(
            &ActorId::parse(actor).unwrap(),
            change(SyncOp::DirState { dir_id: DirId::parse(dir_id).unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Shared").unwrap() } }, pt),
        );
    }
    existing.pull(&server);

    // The winning directory leaves the contested path. The suppressed raw
    // value for dir-a is still live and should become visible again.
    server.accept(
        &ActorId::parse("renamer").unwrap(),
        change(SyncOp::DirState { dir_id: DirId::parse("dir-b").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Renamed").unwrap() } }, 20),
    );
    existing.pull(&server);

    let mut fresh = Device::new("fresh-observer");
    fresh.pull(&server);
    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, name, deleted_at IS NOT NULL FROM dir WHERE id IN ('dir-a','dir-b') ORDER BY id").unwrap();
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&existing), state(&fresh), "derived same-name suppression must be reversible after the winner moves away");
}

#[test]
fn cloned_replicas_with_equal_versions_choose_one_path_occupant() {
    let mut server = FakeServer::default();
    let mut a = Device::new("cloned-device");
    let mut b = Device::new("cloned-device");
    let first_book = BookId::new(1);
    let second_book = BookId::new(2);
    let first_hash = ContentHash::parse(&"1".repeat(64)).unwrap();
    let second_hash = ContentHash::parse(&"2".repeat(64)).unwrap();
    for device in [&a, &b] {
        device.seed_dir("shelf", "Shelf");
        device.seed_book(first_book);
        device.seed_book(second_book);
    }
    a.seed_placement(first_book, "shelf", "same.epub", &first_hash, Hlc { pt: 100, l: 0 });
    a.queue(&placement_add(first_book, "shelf", "same.epub", &first_hash, 100));
    a.seed_placement(first_book, "shelf", "same.epub", &first_hash, Hlc { pt: 100, l: 0 });
    b.seed_placement(second_book, "shelf", "same.epub", &second_hash, Hlc { pt: 100, l: 0 });
    b.queue(&placement_add(second_book, "shelf", "same.epub", &second_hash, 100));
    b.seed_placement(second_book, "shelf", "same.epub", &second_hash, Hlc { pt: 100, l: 0 });

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    let owner = |device: &Device| -> String { device.conn.query_row("SELECT book_id FROM book_dir WHERE dir_id='shelf' AND file_name='same.epub' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap() };
    assert_eq!(owner(&a), owner(&b), "event identity must break a fully cloned VersionKey tie");
}

#[test]
fn exact_hlc_removal_makes_add_blob_collectable() {
    let book_id = BookId::new(1);
    let content_hash = ContentHash::parse(&"e".repeat(64)).unwrap();
    let mut server = FakeServer::default();

    // Arrival order and actor ordering both favor the add, but the protocol's
    // conflict rank deliberately makes a removal win an exact-HLC tie.
    server.accept(&ActorId::parse("a-remover").unwrap(), placement_remove(book_id, "d1", "book.epub", 100));
    server.accept(&ActorId::parse("z-adder").unwrap(), placement_add(book_id, "d1", "book.epub", &content_hash, 100));

    assert!(!server.live_blob_hashes_by_version().contains(&content_hash));
}

#[test]
fn exact_hlc_adds_keep_only_total_version_winner_blob() {
    let book_id = BookId::new(1);
    let winning_hash = ContentHash::parse(&"f".repeat(64)).unwrap();
    let losing_hash = ContentHash::parse(&"0".repeat(64)).unwrap();
    let mut server = FakeServer::default();

    // The lower actor arrives later; actor identity, not arrival, breaks the
    // otherwise exact add/add tie.
    server.accept(&ActorId::parse("z-device").unwrap(), placement_add(book_id, "d1", "book.epub", &winning_hash, 100));
    server.accept(&ActorId::parse("a-device").unwrap(), placement_add(book_id, "d1", "book.epub", &losing_hash, 100));

    assert_eq!(server.live_blob_hashes_by_version(), HashSet::from([winning_hash]));
}

#[test]
fn reading_registers_survive_delete_and_readd_regardless_of_origin() {
    let book_id = BookId::new(1);
    let reader_actor = ActorId::parse("offline-reader").unwrap();
    let mut server = FakeServer::default();
    let mut origin = Device::new(reader_actor.as_str());
    let mut observer = Device::new("observer");
    for device in [&origin, &observer] {
        device.seed_book(book_id);
    }

    // The reader changes both fields while offline, before learning that
    // another replica deleted the book. Its own values are already materialized
    // locally, and production pull excludes events from the requesting actor.
    origin.remote_sql("UPDATE book SET read_pos=?2, last_read=?3 WHERE id=?1", params![book_id.canonical(), "epubcfi(/6/4)", 30_i64]);

    // Commit order at the server is delete -> offline reading fields -> re-add.
    // The origin excludes its own two singleton rows and retains the fields
    // through delete/re-add. The observer applies them while the book is
    // deleted, where the current reducer silently updates zero rows.
    server.accept(&ActorId::parse("deleter").unwrap(), change(SyncOp::BookDeleted { book_id }, 20));
    server.accept(&reader_actor, change(SyncOp::ReadingPositionUpdated { book_id, cfi: "epubcfi(/6/4)".to_string() }, 30));
    server.accept(&reader_actor, change(SyncOp::BookOpened { book_id, last_read: 30 }, 31));
    server.accept(&ActorId::parse("restorer").unwrap(), change(SyncOp::BookAdded { book_id, title: "Restored".to_string(), added_at: 40 }, 40));

    origin.pull(&server);
    observer.pull(&server);

    assert!(origin.book_is_live(book_id) && observer.book_is_live(book_id));
    assert_eq!(origin.reading_state(book_id), observer.reading_state(book_id), "delivery through a tombstoned book must not discard independent scalar registers");
}

#[test]
fn concurrent_directory_moves_cannot_form_a_cycle() {
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_dir("a", "A");
    device.seed_dir("b", "B");

    server.accept(&ActorId::parse("move-a").unwrap(), change(SyncOp::DirState { dir_id: DirId::parse("a").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("b").unwrap(), name: FileName::parse("A").unwrap() } }, 10));
    server.accept(&ActorId::parse("move-b").unwrap(), change(SyncOp::DirState { dir_id: DirId::parse("b").unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("a").unwrap(), name: FileName::parse("B").unwrap() } }, 11));

    device.pull(&server);
    let a_parent: String = device.conn.query_row("SELECT parent_id FROM dir WHERE id='a'", [], |row| row.get(0)).unwrap();
    let b_parent: String = device.conn.query_row("SELECT parent_id FROM dir WHERE id='b'", [], |row| row.get(0)).unwrap();
    assert!(!(a_parent == "b" && b_parent == "a"), "a committed directory projection must remain acyclic");
}

#[test]
fn concurrent_same_name_directories_have_one_physical_owner() {
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    for (actor, dir_id, pt) in [("creator-a", "dir-a", 10), ("creator-b", "dir-b", 11)] {
        server.accept(
            &ActorId::parse(actor).unwrap(),
            change(SyncOp::DirState { dir_id: DirId::parse(dir_id).unwrap(), directory: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Shared").unwrap() } }, pt),
        );
    }

    device.pull(&server);
    let occupants: i64 = device.conn.query_row("SELECT COUNT(*) FROM dir WHERE parent_id='__root__' AND name='Shared' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(occupants, 1, "one filesystem directory path cannot represent two live directory identities");
}

#[test]
fn concurrent_book_adds_have_one_live_occupant_per_physical_path() {
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_dir("shelf", "Shelf");
    let first_hash = ContentHash::parse(&"1".repeat(64)).unwrap();
    let second_hash = ContentHash::parse(&"2".repeat(64)).unwrap();

    server.accept(&ActorId::parse("book-a").unwrap(), placement_add(BookId::new(1), "shelf", "same.epub", &first_hash, 10));
    server.accept(&ActorId::parse("book-b").unwrap(), placement_add(BookId::new(2), "shelf", "same.epub", &second_hash, 11));
    device.pull(&server);

    let occupants: i64 = device.conn.query_row("SELECT COUNT(*) FROM book_dir WHERE dir_id='shelf' AND file_name='same.epub' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(occupants, 1, "one filesystem file path cannot contain two different live books");
}

#[test]
fn cloned_replica_identity_does_not_deduplicate_distinct_changes() {
    let mut server = FakeServer::default();
    let mut first = Device::new("cloned-device");
    let mut second = Device::new("cloned-device");
    let first_book = BookId::new(1);
    let second_book = BookId::new(2);
    first.seed_book(first_book);
    second.seed_book(second_book);
    first.queue(&change(SyncOp::BookAdded { book_id: first_book, title: "First".to_string(), added_at: 1 }, 10));
    second.queue(&change(SyncOp::BookAdded { book_id: second_book, title: "Second".to_string(), added_at: 2 }, 11));

    // Both freshly copied databases allocate local_seq=1. Production append
    // deduplication treats (actor, sequence) as the event identity.
    first.push(&mut server);
    second.push(&mut server);

    assert_eq!(server.append.len(), 2, "distinct mutations from cloned replicas must not collapse to one append event");
}

#[test]
fn malformed_semantic_event_cannot_poison_the_pull_cursor() {
    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_book(book_id);

    // SyncOp currently gives CFI no validated wire type, so this mirrors a
    // production insert that passes deserialization and server range checks.
    server.accept(&ActorId::parse("buggy-client").unwrap(), change(SyncOp::ReadingPositionUpdated { book_id, cfi: "not-a-cfi".to_string() }, 10));
    server.accept(&ActorId::parse("healthy-client").unwrap(), change(SyncOp::BookOpened { book_id, last_read: 20 }, 20));

    let result = device.try_pull(&server);
    assert!(result.is_ok(), "one semantically invalid mutation must be rejected before it can block every later server sequence: {result:?}");
    assert_eq!(device.cursor, 1, "the valid event after a rejected mutation must remain reachable");
}

#[test]
fn book_metadata_updates_produce_a_synchronizable_mutation() {
    let book_id = BookId::new(1);
    let mut device = Device::new("metadata-device");
    device.seed_book(book_id);

    let tx = device.conn.transaction().unwrap();
    db::update_book_metadata(&tx, book_id, "Changed title", "Changed author").unwrap();
    tx.commit().unwrap();

    let outbox = db::get_unpushed_events(&device.conn).unwrap();
    assert!(!outbox.is_empty(), "a metadata change must be represented in the synchronization protocol");
}

#[test]
fn metadata_register_is_not_overwritten_by_lifecycle_readd() {
    let book_id = BookId::new(1);
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_book(book_id);

    server.accept(&ActorId::parse("metadata-writer").unwrap(), change(SyncOp::BookMetadataUpdated { book_id, title: "Canonical title".to_string(), author: "Canonical author".to_string() }, 20));
    server.accept(&ActorId::parse("deleter").unwrap(), change(SyncOp::BookDeleted { book_id }, 30));
    server.accept(&ActorId::parse("restorer").unwrap(), change(SyncOp::BookAdded { book_id, title: "Stale lifecycle title".to_string(), added_at: 40 }, 40));
    device.pull(&server);

    let metadata: (String, String) = device.conn.query_row("SELECT b.title, COALESCE(a.name,'') FROM book b LEFT JOIN author a ON a.id=b.author_id WHERE b.id=?1", [book_id.canonical()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(metadata, ("Canonical title".to_string(), "Canonical author".to_string()));
}

#[test]
fn retry_reads_keep_the_same_persisted_event_identity() {
    let book_id = BookId::new(1);
    let device = Device::new("retry-device");
    device.seed_book(book_id);
    device.queue(&change(SyncOp::BookDeleted { book_id }, 10));

    let first = db::get_unpushed_events(&device.conn).unwrap();
    let second = db::get_unpushed_events(&device.conn).unwrap();
    assert_eq!(first[0].event_id, second[0].event_id, "network retries must reuse the logical mutation identity");
}

#[test]
fn local_directory_api_rejects_cycle_creation() {
    let mut device = Device::new("local-device");
    device.seed_dir("a", "A");
    device.seed_dir("b", "B");
    device.remote_sql("UPDATE dir SET parent_id='a' WHERE id='b'", []);

    let tx = device.conn.transaction().unwrap();
    let result = db::upsert_dir(&tx, &DirId::parse("a").unwrap(), &DirId::parse("b").unwrap(), &FileName::parse("A").unwrap());
    assert!(result.is_err());

    let root_result = db::upsert_dir(&tx, &DirId::parse("__root__").unwrap(), &DirId::parse("a").unwrap(), &FileName::parse("Root").unwrap());
    assert!(root_result.is_err(), "the reserved root cannot become an ordinary child");
}

#[test]
fn local_path_replacement_can_restore_a_previously_tombstoned_occupant() {
    let mut device = Device::new("scanner-device");
    let first_book = BookId::new(1);
    let returning_book = BookId::new(2);
    let first_hash = ContentHash::parse(&"1".repeat(64)).unwrap();
    let returning_hash = ContentHash::parse(&"2".repeat(64)).unwrap();
    device.seed_dir("shelf", "Shelf");
    device.seed_book(first_book);
    device.seed_book(returning_book);
    device.seed_placement(first_book, "shelf", "same.epub", &first_hash, Hlc { pt: 20, l: 0 });
    device.remote_sql(
        "INSERT INTO book_dir(dir_id,book_id,file_name,local_hash,content_hash,deleted_at)
         VALUES ('shelf','2','same.epub','old',?1,1)",
        [returning_hash.as_str()],
    );

    let tx = device.conn.transaction().unwrap();
    let result = db::insert_local_book_dir(&tx, &DirId::parse("shelf").unwrap(), returning_book, &FileName::parse("same.epub").unwrap(), 42, &returning_hash, 2);
    assert!(result.is_ok(), "a final observed path state must replace the live occupant even when its identity has a tombstone: {result:?}");
}

#[derive(Default)]
struct CommitOrderServer {
    next_sequence: u64,
    committed: Vec<(u64, &'static str)>,
}

impl CommitOrderServer {
    fn commit(&mut self, value: &'static str) {
        self.next_sequence += 1;
        let sequence = self.next_sequence;
        self.committed.push((sequence, value));
    }

    fn fetch(&self, cursor: u64) -> Vec<(u64, &'static str)> {
        let mut rows: Vec<_> = self.committed.iter().copied().filter(|(sequence, _)| *sequence > cursor).collect();
        rows.sort_by_key(|(sequence, _)| *sequence);
        rows
    }
}

#[test]
fn server_sequence_cursor_cannot_skip_late_commit() {
    let mut server = CommitOrderServer::default();
    // Revision assignment happens while holding the database transaction lock,
    // immediately before the corresponding commit becomes visible.
    server.commit("second transaction commits first");
    let first_pull = server.fetch(0);
    let cursor = first_pull.last().map(|row| row.0).unwrap_or(0);

    server.commit("first transaction commits late");
    let second_pull = server.fetch(cursor);
    let observed: Vec<_> = first_pull.into_iter().chain(second_pull).collect();

    assert_eq!(observed.len(), 2, "a committed event must never land behind the cursor");
}
