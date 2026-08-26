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

use crate::{db, ContentHash};
use library_model::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState, BookMetadataState, ReadingPositionState};
use library_model::{Contributor, MarcRelatorCode};
use library_replica::{MutationBody, StateKey, StateMutation, VersionKey};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use sync_common::{LibraryRevision, MutationId, ReplicaId, ReplicaSeq, ServerMutation};
use test_support::{fixture_content_hash, fixture_dir_id, fixture_replica_id};

#[derive(Clone)]
struct StoredEvent {
    change: StateMutation,
    replica_id: ReplicaId,
    revision: LibraryRevision,
}

#[derive(Default)]
struct FakeServer {
    next_server_seq: u64,
    mutation_ids: HashSet<MutationId>,
    state: HashMap<StateKey, StoredEvent>,
}

trait FixtureReplica {
    fn fixture_replica(&self) -> ReplicaId;
}

impl FixtureReplica for ReplicaId {
    fn fixture_replica(&self) -> ReplicaId {
        *self
    }
}

impl FixtureReplica for str {
    fn fixture_replica(&self) -> ReplicaId {
        fixture_replica_id(self)
    }
}

impl FixtureReplica for String {
    fn fixture_replica(&self) -> ReplicaId {
        fixture_replica_id(self)
    }
}

impl FakeServer {
    fn push(&mut self, replica_id: &ReplicaId, changes: Vec<StateMutation>) {
        for change in changes {
            self.accept(replica_id, change);
        }
    }

    fn accept<R: FixtureReplica + ?Sized>(&mut self, replica_id: &R, change: StateMutation) {
        let replica_id = replica_id.fixture_replica();
        if !self.mutation_ids.insert(change.mutation_id) {
            return;
        }
        // Match the production server: request identity, not client JSON,
        // determines the authoritative actor and actor-local sequence.
        // `server_seq` is not part of VersionKey, so it is allocated only after
        // the merge decision, exactly as the sequence default does in storage.
        let candidate = StoredEvent { change, replica_id, revision: LibraryRevision::new(1).expect("positive server sequence") };
        let key = candidate.change.body.state_key();
        let candidate_version = VersionKey::new(candidate.change.changed_at, candidate.change.body.conflict_rank(), candidate.replica_id.clone(), candidate.change.replica_seq.get(), candidate.change.mutation_id);
        let incoming_wins =
            self.state.get(&key).is_none_or(|stored| candidate_version > VersionKey::new(stored.change.changed_at, stored.change.body.conflict_rank(), stored.replica_id.clone(), stored.change.replica_seq.get(), stored.change.mutation_id));
        self.next_server_seq += 1;
        let revision = LibraryRevision::new(self.next_server_seq).expect("positive server sequence");
        if incoming_wins {
            self.state.insert(key, StoredEvent { revision, ..candidate });
        } else {
            // Mirrors `enforce_lww_order`: a strictly losing mutation is still
            // acknowledged as accepted, so the pushing replica cannot tell that
            // its local value was discarded. Republishing the stored winner
            // above every cursor is what teaches it.
            self.state.get_mut(&key).expect("a losing candidate has a stored winner").revision = revision;
        }
    }

    fn fetch(&self, _replica_id: &ReplicaId, cursor: u64) -> Vec<ServerMutation> {
        let mut rows: Vec<&StoredEvent> = self.state.values().collect();
        rows.sort_by_key(|row| row.revision.get());
        rows.into_iter().filter(|row| row.revision.get() > cursor).map(|row| ServerMutation { mutation: row.change.to_wire().unwrap(), replica_id: row.replica_id.clone(), revision: row.revision }).collect()
    }

    fn all_rows(&self) -> Vec<&StoredEvent> {
        self.state.values().collect()
    }

    fn winning_position(&self, content_hash: ContentHash) -> String {
        match &self.state.get(&StateKey::ReadingPosition(content_hash)).expect("a stored reading position").change.body {
            MutationBody::ReadingPosition { value, .. } => value.location.as_str().to_owned(),
            other => panic!("reading position cell holds {other:?}"),
        }
    }

    /// Mirrors placement resolution with the protocol's total VersionKey,
    /// independently of server arrival order.
    fn live_books_by_version(&self) -> HashSet<ContentHash> {
        self.all_rows()
            .into_iter()
            .filter_map(|row| match &row.change.body {
                MutationBody::Placement { present: true, content_hash, .. } => Some(content_hash.clone()),
                _ => None,
            })
            .collect()
    }
}

struct Device {
    replica_id: ReplicaId,
    cursor: u64,
    conn: Connection,
}

impl Device {
    fn new(replica_id: &str) -> Self {
        let conn = Connection::open_in_memory().expect("open device database");
        db::init_db(&conn).unwrap();
        // Production passes the client id persisted in this same database to
        // every push. Keep the modeled request identity and trigger-stamped
        // local identity identical as well.
        let replica_id = fixture_replica_id(replica_id);
        db::set_sync_metadata(&conn, "replica_id", &replica_id.to_string()).unwrap();
        Self { replica_id, cursor: 0, conn }
    }

    fn seed_book(&self, content_hash: ContentHash) {
        self.remote_sql(
            "INSERT OR IGNORE INTO book(content_hash, title, added_at, deleted_at)
             VALUES (?1, 'seed', 1, NULL)",
            params![content_hash.as_str()],
        );
    }

    fn seed_dir(&self, dir_id: &str, name: &str) {
        self.remote_sql(
            "INSERT OR IGNORE INTO dir(id, parent_id, name, deleted_at)
             VALUES (?1, '00000000-0000-0000-0000-000000000000', ?2, NULL)",
            params![fixture_dir_id(dir_id).to_string(), name],
        );
    }

    fn seed_placement(&self, content_hash: ContentHash, dir_id: &str, file_name: &str, changed_at: u64) {
        let event_id = "11111111-1111-5111-8111-111111111111";
        // A placement is (book, directory); the file name is local-only and the
        // content identity belongs to the book, not to the placement.
        self.remote_sql(
            "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded, last_scan, deleted_at)
             VALUES (?1, (SELECT row_id FROM book WHERE content_hash=?2), ?3, 'fingerprint', 1, 1, NULL)
             ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
                 file_name=excluded.file_name,
                 deleted_at=NULL",
            params![fixture_dir_id(dir_id).to_string(), content_hash.as_str(), file_name],
        );
        self.force_state_version("placement", content_hash.as_str(), &fixture_dir_id(dir_id).to_string(), changed_at, 0, "0-legacy", 1, event_id);
    }

    /// Writes the durable winning version for one state cell. Merge ordering
    /// lives entirely in `sync_state_version`; domain tables hold materialized
    /// values only, so a test that wants a deterministic winner sets it here.
    #[allow(clippy::too_many_arguments)]
    fn force_state_version(&self, state_kind: &str, state_key: &str, state_subkey: &str, changed_at: u64, conflict_rank: i64, replica_id: &str, replica_seq: i64, mutation_id: &str) {
        self.conn
            .execute(
                "INSERT INTO sync_state_version(state_kind, state_key, state_subkey, changed_at, conflict_rank, replica_id, replica_seq, mutation_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
                     changed_at=excluded.changed_at,
                     conflict_rank=excluded.conflict_rank,
                     replica_id=excluded.replica_id,
                     replica_seq=excluded.replica_seq,
                     mutation_id=excluded.mutation_id",
                params![state_kind, state_key, state_subkey, changed_at as i64, conflict_rank, fixture_replica_id(replica_id).to_string(), replica_seq, mutation_id],
            )
            .expect("force deterministic state version");
    }

    fn set_reading_position(&self, content_hash: ContentHash, cfi: &str) {
        self.remote_sql("UPDATE book SET read_pos=?2 WHERE content_hash=?1", params![content_hash.as_str(), cfi]);
    }

    fn set_book_deleted(&self, content_hash: ContentHash, deleted: bool) {
        let deleted_at = deleted.then_some(1_i64);
        self.remote_sql("UPDATE book SET deleted_at=?2 WHERE content_hash=?1", params![content_hash.as_str(), deleted_at]);
    }

    #[allow(dead_code)]
    fn set_placement_deleted(&self, content_hash: ContentHash, dir_id: &str, deleted: bool, changed_at: u64) {
        let deleted_at = deleted.then_some(1_i64);
        let dir_id = fixture_dir_id(dir_id).to_string();
        self.remote_sql("UPDATE book_dir SET deleted_at=?3 WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", params![dir_id, content_hash.as_str(), deleted_at]);
        self.force_state_version("placement", content_hash.as_str(), &dir_id, changed_at, i64::from(deleted), "0-legacy", 1, "11111111-1111-5111-8111-111111111112");
    }

    fn queue(&self, change: &StateMutation) {
        let (state_kind, state_key) = outbox_storage_key(&change.body);
        let state_subkey = outbox_storage_subkey(&change.body);
        self.conn
            .execute(
                "INSERT INTO sync_outbox(mutation_id, state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
                    replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
                    body=excluded.body, changed_at=excluded.changed_at",
                params![change.mutation_id.to_string(), state_kind, state_key, state_subkey, sync_common::wire::encode(&change.body).unwrap(), i64::try_from(change.changed_at).unwrap(), change.body.conflict_rank()],
            )
            .expect("queue local mutation");
        let local_seq = self.conn.last_insert_rowid();
        if let MutationBody::BookLifecycle { content_hash, .. } = &change.body {
            let replica_id = self.replica_id.to_string();
            self.force_state_version("book_lifecycle", content_hash.as_str(), "", change.changed_at, change.body.conflict_rank() as i64, &replica_id, local_seq, &change.mutation_id.to_string());
        }
    }

    fn push(&mut self, server: &mut FakeServer) {
        let changes = db::get_pending_mutations(&self.conn).expect("decode local outbox");
        let acknowledged = changes.iter().map(|change| change.mutation_id).collect::<Vec<_>>();
        server.push(&self.replica_id, changes);
        db::acknowledge_mutations(&self.conn, &acknowledged).unwrap();
    }

    fn pull(&mut self, server: &FakeServer) {
        self.try_pull(server).expect("apply remote change");
    }

    fn try_pull(&mut self, server: &FakeServer) -> Result<(), Box<dyn std::error::Error>> {
        let changes = server.fetch(&self.replica_id, self.cursor);
        if changes.is_empty() {
            return Ok(());
        }

        let next_cursor = changes.last().expect("non-empty changes").revision.get();
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

    fn has_pending_mutations(&self) -> bool {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox)", [], |row| row.get(0)).expect("outbox state")
    }

    fn read_position(&self, content_hash: ContentHash) -> String {
        self.conn.query_row("SELECT read_pos FROM book WHERE content_hash=?1", params![content_hash.as_str()], |row| row.get(0)).expect("book read position")
    }

    fn book_is_live(&self, content_hash: ContentHash) -> bool {
        self.conn.query_row("SELECT deleted_at IS NULL FROM book WHERE content_hash=?1", params![content_hash.as_str()], |row| row.get(0)).expect("book liveness")
    }

    /// Content identity belongs to the book; a placement only records membership
    /// in a directory. Returns None when the placement is not live, so callers
    /// must distinguish "no placement" from "placement with unknown bytes".
    fn placement_hash(&self, content_hash: ContentHash, dir_id: &str) -> Option<ContentHash> {
        self.conn
            .query_row(
                "SELECT book.content_hash FROM book_dir
                 JOIN book ON book.row_id = book_dir.book_row_id
                 WHERE book_dir.dir_id=?1 AND book.content_hash=?2
                   AND book_dir.deleted_at IS NULL",
                params![fixture_dir_id(dir_id).to_string(), content_hash.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .expect("placement lookup")
            .map(|hash| ContentHash::new(&hash))
    }

    fn live_placements(&self, dir_id: &str) -> Vec<ContentHash> {
        let mut statement =
            self.conn.prepare("SELECT book.content_hash FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id WHERE dir_id=?1 AND book_dir.deleted_at IS NULL ORDER BY book.content_hash").expect("prepare placement query");
        let ids = statement.query_map(params![fixture_dir_id(dir_id).to_string()], |row| row.get::<_, String>(0)).expect("query placements").collect::<Result<Vec<_>, _>>().expect("read placements");
        ids.iter().map(|id| ContentHash::new(&id.as_str())).collect()
    }

    fn placement_is_live(&self, content_hash: ContentHash, dir_id: &str) -> bool {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id WHERE dir_id=?1 AND book.content_hash=?2 AND book_dir.deleted_at IS NULL)",
                params![fixture_dir_id(dir_id).to_string(), content_hash.as_str()],
                |row| row.get(0),
            )
            .expect("placement liveness")
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

fn outbox_storage_key(body: &MutationBody) -> (&'static str, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string()),
        MutationBody::Placement { content_hash, .. } => ("placement", content_hash.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone()),
        MutationBody::Bookmark { bookmark_id, .. } => ("bookmark", bookmark_id.clone()),
        MutationBody::BookLink { link_id, .. } => ("book_link", link_id.clone()),
        MutationBody::CatalogPublication { source_id, .. } => ("catalog_publication", source_id.as_str().to_owned()),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string()),
    }
}

fn outbox_storage_subkey(body: &MutationBody) -> String {
    match body {
        MutationBody::Placement { dir_id, .. } => dir_id.to_string(),
        MutationBody::CatalogPublication { publication, .. } => library_replica::catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref()),
        MutationBody::DirectoryName { .. }
        | MutationBody::DirectoryParent { .. }
        | MutationBody::DirectoryLifecycle { .. }
        | MutationBody::ReadingPosition { .. }
        | MutationBody::Annotation { .. }
        | MutationBody::Bookmark { .. }
        | MutationBody::BookLink { .. }
        | MutationBody::Metadata { .. }
        | MutationBody::BookLifecycle { .. } => String::new(),
    }
}

fn change(body: MutationBody, pt: u64) -> StateMutation {
    StateMutation { mutation_id: MutationId::new(), body, changed_at: pt, replica_seq: ReplicaSeq::new(1).unwrap() }
}

fn metadata(title: impl Into<String>, authors: Vec<Contributor>, description: impl Into<String>) -> BookMetadataState {
    BookMetadataState { title: title.into(), contributors: authors, description: description.into(), publication: Default::default() }
}

fn placement_add(content_hash: ContentHash, dir_id: &str, _file_name: &str, _content_hash: &ContentHash, pt: u64) -> StateMutation {
    change(MutationBody::Placement { dir_id: fixture_dir_id(dir_id), content_hash, present: true, origin_folder_id: None }, pt)
}

fn placement_remove(content_hash: ContentHash, dir_id: &str, _file_name: &str, pt: u64) -> StateMutation {
    change(MutationBody::Placement { dir_id: fixture_dir_id(dir_id), content_hash, present: false, origin_folder_id: None }, pt)
}

fn reading_position(content_hash: ContentHash, location: impl Into<String>, progress: f32, pt: u64) -> StateMutation {
    let location = location.into();
    change(MutationBody::ReadingPosition { content_hash, value: ReadingPositionState { location: library_model::ReadingPosition::parse(&location).unwrap(), progress } }, pt)
}

fn book_present(content_hash: ContentHash, title: impl Into<String>, added_at: u64, pt: u64) -> StateMutation {
    change(MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Present { metadata: metadata(title, Vec::new(), ""), added_at, format: BookFormat::Epub } }, pt)
}

fn book_deleted(content_hash: ContentHash, pt: u64) -> StateMutation {
    change(MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Deleted { origin_folder_id: None } }, pt)
}

fn directory_present(dir_id: &str, parent_id: &str, name: &str, pt: u64) -> [StateMutation; 3] {
    let dir_id = fixture_dir_id(dir_id);
    let parent_id = fixture_dir_id(parent_id);
    [
        change(MutationBody::DirectoryName { dir_id: dir_id.clone(), name: name.to_owned() }, pt),
        change(MutationBody::DirectoryParent { dir_id: dir_id.clone(), parent_id }, pt),
        change(MutationBody::DirectoryLifecycle { dir_id, value: library_model::DirectoryLifecycleState::Present }, pt),
    ]
}

fn accept_directory(server: &mut FakeServer, actor: &str, dir_id: &str, parent_id: &str, name: &str, pt: u64) {
    let actor = fixture_replica_id(actor);
    for change in directory_present(dir_id, parent_id, name, pt) {
        server.accept(&actor, change);
    }
}

#[test]
fn fake_server_deduplicates_append_retries() {
    let mut server = FakeServer::default();
    let event = book_present(fixture_content_hash(1), "Book", 1, 1);

    let replica_id = "device".to_owned();
    server.accept(&replica_id, event.clone());
    server.accept(&replica_id, event);

    assert_eq!(server.state.len(), 1);
}

#[test]
fn equal_wall_time_singleton_conflict_converges() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    // z wins the server's exact-wall-time replica-id tie.
    let mut a = Device::new("z-device");
    let mut b = Device::new("a-device");
    for device in [&a, &b] {
        device.seed_book(content_hash);
    }

    let a_change = reading_position(content_hash, "epubcfi(/6/2)", 20.0, 100);
    a.set_reading_position(content_hash, "epubcfi(/6/2)");
    a.queue(&a_change);

    let b_change = reading_position(content_hash, "epubcfi(/6/4)", 40.0, 100);
    b.set_reading_position(content_hash, "epubcfi(/6/4)");
    b.queue(&b_change);

    b.push(&mut server);
    a.pull(&server);
    a.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.read_position(content_hash), b.read_position(content_hash));
}

/// Devices do not share a clock, and the service deliberately admits timestamps
/// up to `MAX_LWW_FUTURE_SKEW_MS` ahead of its own. A replica can therefore
/// adopt a remote value stamped later than its own next honest local write, and
/// that local write then loses the server's merge.
///
/// The push acknowledgement cannot distinguish "applied" from "merged and
/// discarded" -- both are reported as accepted -- and the trigger that stamps
/// `sync_state_version` for a local write is unconditional, so the loser keeps
/// its own value with no outbox entry left to retry. The winning row already
/// sits below its pull cursor, so nothing would ever redeliver it. The service
/// must republish the winner above every cursor instead.
#[test]
fn a_local_write_that_loses_the_servers_merge_is_repaired_without_a_retry() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut ahead = Device::new("clock-ahead");
    let mut behind = Device::new("clock-behind");
    for device in [&ahead, &behind] {
        device.seed_book(content_hash);
    }

    // The fast-clocked device publishes a position stamped in the other's future.
    ahead.set_reading_position(content_hash, "epubcfi(/6/2)");
    ahead.queue(&reading_position(content_hash, "epubcfi(/6/2)", 20.0, 2_000));
    ahead.push(&mut server);
    behind.pull(&server);
    assert_eq!(behind.read_position(content_hash), "epubcfi(/6/2)", "the remote value is adopted before the local write");

    // An honest local write on the slow-clocked device, which the service will
    // discard because 1_000 loses to the stored 2_000.
    behind.set_reading_position(content_hash, "epubcfi(/6/4)");
    behind.queue(&reading_position(content_hash, "epubcfi(/6/4)", 40.0, 1_000));
    behind.push(&mut server);
    assert!(!behind.has_pending_mutations(), "an accepted push leaves nothing to retry");

    behind.pull(&server);
    ahead.pull(&server);

    assert_eq!(behind.read_position(content_hash), "epubcfi(/6/2)", "the discarded local write must not survive as a divergent replica value");
    assert_eq!(ahead.read_position(content_hash), behind.read_position(content_hash));
    assert_eq!(server.winning_position(content_hash), "epubcfi(/6/2)");
}

/// The repair must be a redelivery, not a rewrite: republishing the winner may
/// not disturb which mutation owns the cell, and a replica that already holds
/// the winner must not be able to lose it to the same stale write arriving again.
#[test]
fn republishing_a_merge_winner_is_idempotent_across_replicas() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut ahead = Device::new("clock-ahead");
    let mut behind = Device::new("clock-behind");
    let mut bystander = Device::new("bystander");
    for device in [&ahead, &behind, &bystander] {
        device.seed_book(content_hash);
    }

    ahead.set_reading_position(content_hash, "epubcfi(/6/2)");
    ahead.queue(&reading_position(content_hash, "epubcfi(/6/2)", 20.0, 2_000));
    ahead.push(&mut server);
    for device in [&mut behind, &mut bystander] {
        device.pull(&server);
    }

    for stale in ["epubcfi(/6/4)", "epubcfi(/6/6)"] {
        behind.set_reading_position(content_hash, stale);
        behind.queue(&reading_position(content_hash, stale, 40.0, 1_000));
        behind.push(&mut server);
        behind.pull(&server);
        bystander.pull(&server);
    }
    ahead.pull(&server);

    for device in [&ahead, &behind, &bystander] {
        assert_eq!(device.read_position(content_hash), "epubcfi(/6/2)");
    }
    assert_eq!(server.all_rows().len(), 1, "a republished winner replaces its own row rather than accumulating");
}

#[test]
fn annotation_updates_and_tombstones_converge() {
    let content_hash = fixture_content_hash(1);
    let annotation_id = "46d3716b-a9ea-4942-830b-cbaf95673dd5".to_owned();
    let mut server = FakeServer::default();
    let mut a = Device::new("annotation-a");
    let mut b = Device::new("annotation-b");
    for device in [&a, &b] {
        device.seed_book(content_hash);
    }

    let state = |note: &str, modified_at: i64, deleted| AnnotationState {
        content_hash,
        anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"),
        exact_text: "Text".to_owned(),
        style: AnnotationStyle::Highlight,
        color: "#f6c945".to_owned(),
        note: note.to_owned(),
        created_at: 1,
        modified_at: u64::try_from(modified_at).unwrap(),
        deleted,
    };
    server.accept(&"writer-a".to_owned(), change(MutationBody::Annotation { annotation_id: annotation_id.clone(), value: state("old", 10, false) }, 10));
    server.accept(&"writer-b".to_owned(), change(MutationBody::Annotation { annotation_id: annotation_id.clone(), value: state("new", 20, false) }, 20));
    a.pull(&server);
    b.pull(&server);
    for device in [&a, &b] {
        let note: String = device.conn.query_row("SELECT note FROM annotation WHERE id=?1", [&annotation_id], |row| row.get(0)).unwrap();
        assert_eq!(note, "new");
    }

    server.accept(&"writer-a".to_owned(), change(MutationBody::Annotation { annotation_id: annotation_id.clone(), value: state("new", 30, true) }, 30));
    a.pull(&server);
    b.pull(&server);
    for device in [&a, &b] {
        let deleted: bool = device.conn.query_row("SELECT deleted_at IS NOT NULL FROM annotation WHERE id=?1", [&annotation_id], |row| row.get(0)).unwrap();
        assert!(deleted);
    }
}

#[test]
fn repeated_partition_heal_cycles_converge_in_both_reconnect_orders() {
    const CYCLES: usize = 24;
    const OFFLINE_UPDATES_PER_CYCLE: usize = 7;

    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut devices = vec![Device::new("partition-a"), Device::new("partition-b"), Device::new("partition-c")];
    let device_count = devices.len();
    for device in &devices {
        device.seed_book(content_hash);
    }

    for cycle in 0..CYCLES {
        let isolated = cycle % device_count;
        let base_time = 1_000 + cycle as u64 * 100;
        let offline_wins = cycle % 2 == 1;

        // The isolated replica accumulates several local singleton updates.
        // Odd cycles give it the greatest version; even cycles deliberately
        // make its entire offline history stale.
        for update in 0..OFFLINE_UPDATES_PER_CYCLE {
            let pt = base_time + update as u64 + if offline_wins { 50 } else { 0 };
            let cfi = format!("epubcfi(/6/{})", cycle * 100 + update + 2);
            devices[isolated].set_reading_position(content_hash, &cfi);
            devices[isolated].queue(&reading_position(content_hash, cfi, 0.0, pt));
        }

        // Connected replicas continue independently while the selected device
        // is partitioned.
        for offset in 1..device_count {
            let actor = (isolated + offset) % device_count;
            let pt = base_time + 20 + offset as u64;
            let cfi = format!("epubcfi(/8/{})", cycle * 10 + offset);
            devices[actor].set_reading_position(content_hash, &cfi);
            devices[actor].queue(&reading_position(content_hash, cfi, 0.0, pt));
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

        let expected = devices[0].read_position(content_hash);
        for (index, device) in devices.iter().enumerate().skip(1) {
            assert_eq!(device.read_position(content_hash), expected, "cycle {cycle}: replica {index} diverged after partition healing");
        }
    }

    for device in &devices {
        assert!(db::get_pending_mutations(&device.conn).unwrap().is_empty(), "healed replica retained acknowledged outbox mutations");
    }
    assert_eq!(server.state.keys().filter(|key| matches!(key, StateKey::ReadingPosition(_))).count(), 1, "repeated offline histories must collapse to one server register");
}

#[test]
fn concurrent_book_add_delete_converges() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_book(content_hash);
    }

    a.queue(&book_present(content_hash, "seed", 1, 100));
    b.set_book_deleted(content_hash, true);
    b.queue(&book_deleted(content_hash, 101));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.book_is_live(content_hash), b.book_is_live(content_hash));
    assert!(!a.book_is_live(content_hash), "the higher-version deletion must win");
}

#[test]
fn equal_wall_time_book_add_delete_converges_to_deleted() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut a = Device::new("z-device");
    let mut b = Device::new("a-device");
    for device in [&a, &b] {
        device.seed_book(content_hash);
    }

    a.queue(&book_present(content_hash, "seed", 1, 100));
    b.set_book_deleted(content_hash, true);
    b.queue(&book_deleted(content_hash, 100));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert!(!a.book_is_live(content_hash));
    assert!(!b.book_is_live(content_hash));
}

#[test]
fn equal_wall_clock_placement_adds_converge() {
    let content_hash = fixture_content_hash(1);
    let a_hash = ContentHash::new(&"a".repeat(64));
    let b_hash = ContentHash::new(&"b".repeat(64));
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(content_hash);
    }

    // A placement records only that this book belongs to this directory, so two
    // devices asserting it at the same wall clock must end up agreeing.
    a.seed_placement(content_hash, "d1", "book.epub", 100);
    a.queue(&placement_add(content_hash, "d1", "book.epub", &a_hash, 100));
    b.seed_placement(content_hash, "d1", "book.epub", 100);
    b.queue(&placement_add(content_hash, "d1", "book.epub", &b_hash, 100));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert!(a.placement_is_live(content_hash, "d1"));
    assert_eq!(a.live_placements("d1"), b.live_placements("d1"));
}

/// Content identity travels with the book lifecycle, never with a placement, so
/// a device that has only ever pulled still learns which bytes the book is.
#[test]
fn book_content_identity_arrives_with_the_lifecycle_register() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(content_hash);
    }

    a.seed_placement(content_hash, "d1", "book.epub", 100);
    a.queue(&book_present(content_hash, "Book", 10, 100));
    a.queue(&placement_add(content_hash, "d1", "book.epub", &content_hash, 100));
    a.push(&mut server);
    b.pull(&server);

    assert_eq!(b.placement_hash(content_hash, "d1"), Some(content_hash), "the pulled lifecycle carries the book's content identity");
}

/// Two different books may occupy one directory at once. The old model made a
/// filename part of placement identity and forced one live occupant per path;
/// names are local-only now, so both placements stay live on every device.
#[test]
fn two_books_placed_in_one_directory_both_stay_live() {
    let first = fixture_content_hash(1);
    let second = fixture_content_hash(2);
    let first_hash = ContentHash::new(&"a".repeat(64));
    let second_hash = ContentHash::new(&"b".repeat(64));
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(first);
        device.seed_book(second);
    }

    a.seed_placement(first, "d1", "book.epub", 100);
    a.queue(&placement_add(first, "d1", "book.epub", &first_hash, 100));
    b.seed_placement(second, "d1", "book.epub", 110);
    b.queue(&placement_add(second, "d1", "book.epub", &second_hash, 110));

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    assert_eq!(a.live_placements("d1"), vec![first, second]);
    assert_eq!(a.live_placements("d1"), b.live_placements("d1"));
}

/// A removal with a newer wall clock wins over the add it follows, and a later
/// add restores the placement rather than being suppressed by its tombstone.
#[test]
fn placement_removal_and_readd_converge_by_wall_clock() {
    let content_hash = fixture_content_hash(1);
    let hash = ContentHash::new(&"a".repeat(64));
    let mut server = FakeServer::default();
    let mut a = Device::new("a-device");
    let mut b = Device::new("b-device");
    for device in [&a, &b] {
        device.seed_dir("d1", "Shelf");
        device.seed_book(content_hash);
    }

    a.seed_placement(content_hash, "d1", "book.epub", 100);
    a.queue(&placement_add(content_hash, "d1", "book.epub", &hash, 100));
    a.push(&mut server);
    b.pull(&server);
    assert!(b.placement_is_live(content_hash, "d1"));

    b.set_placement_deleted(content_hash, "d1", true, 200);
    b.queue(&placement_remove(content_hash, "d1", "book.epub", 200));
    b.push(&mut server);
    a.pull(&server);
    assert!(!a.placement_is_live(content_hash, "d1"), "a newer removal wins over the add it follows");

    a.seed_placement(content_hash, "d1", "book.epub", 300);
    a.queue(&placement_add(content_hash, "d1", "book.epub", &hash, 300));
    a.push(&mut server);
    b.pull(&server);
    assert!(b.placement_is_live(content_hash, "d1"), "a newer add restores a tombstoned placement");
    assert_eq!(a.live_placements("d1"), b.live_placements("d1"));
}

#[test]
fn stale_removal_cannot_collect_winning_add_blob() {
    let content_hash = ContentHash::new(&"d".repeat(64));
    let mut server = FakeServer::default();

    server.accept(&"newer-device".to_owned(), placement_add(content_hash, "d1", "book.epub", &content_hash, 200));
    server.accept(&"stale-device".to_owned(), placement_remove(content_hash, "d1", "book.epub", 100));

    assert!(server.live_books_by_version().contains(&content_hash), "the live placement must win despite stale removal");
}

#[test]
fn removal_of_a_displaced_book_cannot_collect_the_replacement_blob() {
    let old_book = fixture_content_hash(1);
    let new_book = fixture_content_hash(2);
    let old_hash = ContentHash::new(&"a".repeat(64));
    let replacement_hash = ContentHash::new(&"b".repeat(64));
    let mut server = FakeServer::default();

    server.accept(&"old-adder".to_owned(), placement_add(old_book, "d1", "book.epub", &old_hash, 10));
    server.accept(&"replacer".to_owned(), placement_add(new_book, "d1", "book.epub", &replacement_hash, 20));
    server.accept(&"late-old-remover".to_owned(), placement_remove(old_book, "d1", "book.epub", 30));

    assert_eq!(server.live_books_by_version(), HashSet::from([new_book]));
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

    let move_a = directory_present("a", "b", "A", 10);
    let move_b = directory_present("b", "a", "B", 11);
    a.remote_sql("UPDATE dir SET parent_id=?1 WHERE id=?2", params![fixture_dir_id("b").to_string(), fixture_dir_id("a").to_string()]);
    for change in &move_a {
        a.queue(change);
    }
    b.remote_sql("UPDATE dir SET parent_id=?1 WHERE id=?2", params![fixture_dir_id("a").to_string(), fixture_dir_id("b").to_string()]);
    for change in &move_b {
        b.queue(change);
    }

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, parent_id, deleted_at IS NOT NULL FROM dir WHERE id IN (?1,?2) ORDER BY id").unwrap();
        statement.query_map(params![fixture_dir_id("a").to_string(), fixture_dir_id("b").to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&a), state(&b), "cycle arbitration must not depend on which move was already materialized locally");
    let a_id = fixture_dir_id("a").to_string();
    let b_id = fixture_dir_id("b").to_string();
    assert!(!state(&a).iter().all(|(_, parent, deleted)| !deleted && (parent == &a_id || parent == &b_id)));
}

#[test]
fn breaking_a_directory_cycle_revives_the_suppressed_directory() {
    let mut server = FakeServer::default();
    let mut existing = Device::new("existing-observer");
    existing.seed_dir("a", "A");
    existing.seed_dir("b", "B");

    accept_directory(&mut server, "move-a", "a", "b", "A", 10);
    accept_directory(&mut server, "move-b", "b", "a", "B", 11);
    existing.pull(&server);

    // Moving A to root breaks the cycle. B's raw register is still the live
    // value `B -> A`, but B receives no new event of its own.
    accept_directory(&mut server, "cycle-breaker", "a", "00000000-0000-0000-0000-000000000000", "A", 20);
    existing.pull(&server);

    // A fresh replica sees only the server's current singleton values. It must
    // derive the same visible tree as a replica that witnessed the old cycle.
    let mut fresh = Device::new("fresh-observer");
    fresh.pull(&server);
    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, parent_id, deleted_at IS NOT NULL FROM dir WHERE id IN (?1,?2) ORDER BY id").unwrap();
        statement.query_map(params![fixture_dir_id("a").to_string(), fixture_dir_id("b").to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&existing), state(&fresh), "derived cycle suppression must be reversible when the cycle is broken");
}

#[test]
fn same_name_directory_resolution_converges_on_the_origin_replicas() {
    let mut server = FakeServer::default();
    let mut a = Device::new("creator-a");
    let mut b = Device::new("creator-b");
    let first = directory_present("dir-a", "00000000-0000-0000-0000-000000000000", "Shared", 10);
    let second = directory_present("dir-b", "00000000-0000-0000-0000-000000000000", "Shared", 11);
    a.remote_sql("INSERT INTO dir(id,parent_id,name) VALUES (?1,'00000000-0000-0000-0000-000000000000','Shared')", [fixture_dir_id("dir-a").to_string()]);
    for change in &first {
        a.queue(change);
    }
    b.remote_sql("INSERT INTO dir(id,parent_id,name) VALUES (?1,'00000000-0000-0000-0000-000000000000','Shared')", [fixture_dir_id("dir-b").to_string()]);
    for change in &second {
        b.queue(change);
    }

    a.push(&mut server);
    b.push(&mut server);
    a.pull(&server);
    b.pull(&server);

    let owner = |device: &Device| -> String { device.conn.query_row("SELECT id FROM dir WHERE parent_id='00000000-0000-0000-0000-000000000000' AND name='Shared' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap() };
    assert_eq!(owner(&a), owner(&b));
}

#[test]
fn renaming_a_same_name_winner_revives_the_suppressed_directory() {
    let mut server = FakeServer::default();
    let mut existing = Device::new("existing-observer");
    for (actor, dir_id, pt) in [("creator-a", "dir-a", 10), ("creator-b", "dir-b", 11)] {
        accept_directory(&mut server, actor, dir_id, "00000000-0000-0000-0000-000000000000", "Shared", pt);
    }
    existing.pull(&server);

    // The winning directory leaves the contested path. The suppressed raw
    // value for dir-a is still live and should become visible again.
    accept_directory(&mut server, "renamer", "dir-b", "00000000-0000-0000-0000-000000000000", "Renamed", 20);
    existing.pull(&server);

    let mut fresh = Device::new("fresh-observer");
    fresh.pull(&server);
    let state = |device: &Device| -> Vec<(String, String, bool)> {
        let mut statement = device.conn.prepare("SELECT id, name, deleted_at IS NOT NULL FROM dir WHERE id IN (?1,?2) ORDER BY id").unwrap();
        statement.query_map(params![fixture_dir_id("dir-a").to_string(), fixture_dir_id("dir-b").to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(state(&existing), state(&fresh), "derived same-name suppression must be reversible after the winner moves away");
}

#[test]
fn equal_wall_clock_adds_keep_only_total_version_winner_blob() {
    let content_hash = fixture_content_hash(1);
    let winning_hash = ContentHash::new(&"f".repeat(64));
    let losing_hash = ContentHash::new(&"0".repeat(64));
    let mut server = FakeServer::default();

    // The lower actor arrives later; actor identity, not arrival, breaks the
    // otherwise exact add/add tie.
    server.accept(&"z-device".to_owned(), placement_add(content_hash, "d1", "book.epub", &winning_hash, 100));
    server.accept(&"a-device".to_owned(), placement_add(content_hash, "d1", "book.epub", &losing_hash, 100));

    assert_eq!(server.live_books_by_version(), HashSet::from([content_hash]));
}

#[test]
fn reading_registers_survive_delete_and_readd_regardless_of_origin() {
    let content_hash = fixture_content_hash(1);
    let reader_actor = "offline-reader".to_owned();
    let mut server = FakeServer::default();
    let mut origin = Device::new(reader_actor.as_str());
    let mut observer = Device::new("observer");
    for device in [&origin, &observer] {
        device.seed_book(content_hash);
    }

    // The reader changes its position while offline, before learning that
    // another replica deleted the book. Its own values are already materialized
    // locally, and production pull excludes events from the requesting actor.
    origin.set_reading_position(content_hash, "epubcfi(/6/4)");

    // Commit order at the server is delete -> offline reading position -> re-add.
    // The origin excludes its own singleton row and retains the position
    // through delete/re-add. The observer applies them while the book is
    // deleted, where the current reducer silently updates zero rows.
    server.accept(&"deleter".to_owned(), book_deleted(content_hash, 20));
    server.accept(&reader_actor, reading_position(content_hash, "epubcfi(/6/4)", 40.0, 30));
    server.accept(&"restorer".to_owned(), book_present(content_hash, "Restored", 40, 40));

    origin.pull(&server);
    observer.pull(&server);

    assert!(origin.book_is_live(content_hash) && observer.book_is_live(content_hash));
    assert_eq!(origin.read_position(content_hash), observer.read_position(content_hash), "delivery through a tombstoned book must not discard the reading-position register");
}

#[test]
fn concurrent_directory_moves_cannot_form_a_cycle() {
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_dir("a", "A");
    device.seed_dir("b", "B");

    accept_directory(&mut server, "move-a", "a", "b", "A", 10);
    accept_directory(&mut server, "move-b", "b", "a", "B", 11);

    device.pull(&server);
    let a_id = fixture_dir_id("a").to_string();
    let b_id = fixture_dir_id("b").to_string();
    let a_parent: String = device.conn.query_row("SELECT parent_id FROM dir WHERE id=?1", [a_id], |row| row.get(0)).unwrap();
    let b_parent: String = device.conn.query_row("SELECT parent_id FROM dir WHERE id=?1", [b_id.as_str()], |row| row.get(0)).unwrap();
    assert!(!(a_parent == b_id && b_parent == fixture_dir_id("a").to_string()), "a committed directory projection must remain acyclic");
}

#[test]
fn concurrent_same_name_directories_have_one_physical_owner() {
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    for (actor, dir_id, pt) in [("creator-a", "dir-a", 10), ("creator-b", "dir-b", 11)] {
        accept_directory(&mut server, actor, dir_id, "00000000-0000-0000-0000-000000000000", "Shared", pt);
    }

    device.pull(&server);
    let occupants: i64 = device.conn.query_row("SELECT COUNT(*) FROM dir WHERE parent_id='00000000-0000-0000-0000-000000000000' AND name='Shared' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(occupants, 1, "one filesystem directory path cannot represent two live directory identities");
}

#[test]
fn cloned_replica_identity_does_not_deduplicate_distinct_changes() {
    let mut server = FakeServer::default();
    let mut first = Device::new("cloned-device");
    let mut second = Device::new("cloned-device");
    let first_book = fixture_content_hash(1);
    let second_book = fixture_content_hash(2);
    first.seed_book(first_book);
    second.seed_book(second_book);
    first.queue(&book_present(first_book, "First", 1, 10));
    second.queue(&book_present(second_book, "Second", 2, 11));

    // Both freshly copied databases allocate local_seq=1. Production append
    // deduplication treats (actor, sequence) as the event identity.
    first.push(&mut server);
    second.push(&mut server);

    assert_eq!(server.state.len(), 2, "distinct mutations from cloned replicas must not collapse to one state cell");
}

#[test]
fn book_metadata_updates_produce_a_synchronizable_mutation() {
    let content_hash = fixture_content_hash(1);
    let mut device = Device::new("metadata-device");
    device.seed_book(content_hash);

    let tx = device.conn.transaction().unwrap();
    let contributors = [Contributor::new("Changed author", None, MarcRelatorCode(*b"aut")).unwrap()];
    db::update_book_metadata_contributors(&tx, content_hash, "Changed title", &contributors).unwrap();
    tx.commit().unwrap();

    let outbox = db::get_pending_mutations(&device.conn).unwrap();
    assert!(!outbox.is_empty(), "a metadata change must be represented in the synchronization protocol");
}

#[test]
fn metadata_register_is_not_overwritten_by_lifecycle_readd() {
    let content_hash = fixture_content_hash(1);
    let mut server = FakeServer::default();
    let mut device = Device::new("observer");
    device.seed_book(content_hash);

    server.accept(
        &"metadata-writer".to_owned(),
        change(MutationBody::Metadata { content_hash, value: metadata("Canonical title", vec![Contributor::new("Canonical author", None, MarcRelatorCode(*b"aut")).unwrap()], "Canonical description") }, 20),
    );
    server.accept(&"deleter".to_owned(), book_deleted(content_hash, 30));
    server.accept(&"restorer".to_owned(), book_present(content_hash, "Stale lifecycle title", 40, 40));
    device.pull(&server);

    let metadata: (String, String, String) = device
        .conn
        .query_row(
            "SELECT b.title, COALESCE((SELECT group_concat(credit.name, ', ') FROM (SELECT name FROM book_contributor WHERE book_row_id=b.row_id AND role=X'617574' ORDER BY position) credit), ''), b.description FROM book b WHERE b.content_hash=?1",
            [content_hash.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(metadata, ("Canonical title".to_string(), "Canonical author".to_string(), "Canonical description".to_string()));
}

#[test]
fn retry_reads_keep_the_same_persisted_event_identity() {
    let content_hash = fixture_content_hash(1);
    let device = Device::new("retry-device");
    device.seed_book(content_hash);
    device.queue(&book_deleted(content_hash, 10));

    let first = db::get_pending_mutations(&device.conn).unwrap();
    let second = db::get_pending_mutations(&device.conn).unwrap();
    assert_eq!(first[0].mutation_id, second[0].mutation_id, "network retries must reuse the logical mutation identity");
}

#[test]
fn local_directory_api_rejects_cycle_creation() {
    let mut device = Device::new("local-device");
    device.seed_dir("a", "A");
    device.seed_dir("b", "B");
    device.remote_sql("UPDATE dir SET parent_id=?1 WHERE id=?2", params![fixture_dir_id("a").to_string(), fixture_dir_id("b").to_string()]);

    let tx = device.conn.transaction().unwrap();
    let result = db::upsert_dir(&tx, &fixture_dir_id("a"), &fixture_dir_id("b"), &"A".to_owned());
    assert!(result.is_err());

    let root_result = db::upsert_dir(&tx, &sync_common::ROOT_DIR_ID, &fixture_dir_id("a"), &"Root".to_owned());
    assert!(root_result.is_err(), "the reserved root cannot become an ordinary child");
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
