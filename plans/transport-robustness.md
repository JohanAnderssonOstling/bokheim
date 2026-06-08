# Transport robustness

## Goal
Make the metadata sync transport survive long offline periods, flaky networks, and server outages without burning battery or stalling the device. Two changes:
1. **Pull pagination** — bound response size so a long offline period doesn't produce a single huge payload that fails as a unit.
2. **Adaptive sync cadence** — replace the hard 30-second loop with debounced event-driven push and exponential backoff on failure.

## Non-goals
- No new auth, no transport encryption changes (separate plan).
- No protocol restructure beyond adding a `limit` parameter and a `has_more` response field.
- No mobile background sync (out of scope per the asset-sync plan).
- No streaming / WebSocket push from server. Polling stays.

## Background

**Pull today** (`Backend/src/sync.rs:299-331`, `Server/src/routes.rs:52`):
- `GET /api/sync/pull?user_id=X&library_id=Y&since_server_seq=N`
- Server returns *all* changes since `since_server_seq` in one `PullChangesResponse`.
- Client applies them in a single transaction; on success, advances `last_pull_server_seq` to the last event's `server_seq`.

Failure modes today:
- After a long offline period (weeks), the response can be megabytes of JSON. A single network drop kills the whole batch and the client retries the same massive payload on the next tick.
- Memory spike on mobile parsing a huge JSON array.
- The single transaction holds a write lock on SQLite for the duration of the apply, blocking UI reads.

**Sync loop today** (`Backend/src/sync.rs:93-103`):
```rust
let mut interval = tokio::time::interval(Duration::from_secs(30));
loop {
    interval.tick().await;
    if let Err(e) = self.sync_with_server().await {
        log::error!("Sync failed: {}", e);
    }
}
```

Failure modes:
- Server down → 30s retry forever, no backoff. Drains battery on mobile.
- Reading-position update at second 1 sits for up to 29 seconds before pushing. User flips to another device, sees stale state.
- No way to trigger a sync immediately on important local events (book added, manual "sync now" button).

## Design

### Pull pagination

Add a `limit` query parameter (default and max enforced server-side). Add a `has_more` field to `PullChangesResponse`. Client loops:

```
loop:
    GET /api/sync/pull?...&since_server_seq=cursor&limit=500
    apply changes in a transaction, commit
    advance cursor to last event's server_seq
    if !has_more: break
```

Each page is its own transaction. A network failure mid-loop loses the *current* unfinished page only; previously-applied pages are durable. The cursor advances incrementally, so the next sync attempt resumes from where the last successful page ended.

Page size: 500 events is a reasonable default. At ~200 bytes/event JSON-encoded that's ~100 KB per page — small enough to retry cheaply, large enough that a 50k-event backlog completes in 100 round trips. Adjustable via query param up to a server-enforced max (say 5000) for desktop clients on fast networks.

### Adaptive sync cadence

Replace the fixed-interval loop with a small state machine driven by a wakeup channel:

```
loop:
    select:
        wakeup signal → sync_once
        timer tick   → sync_once
    update next_tick based on success/failure
```

**Triggers for wakeup:**
- A debounced channel from the trigger path: any `event_log` insert pings the channel; a 2-second debounce window batches rapid changes (e.g., reading-position updates as the user scrolls). Push fires after the debounce settles.
- Manual user action ("Sync now" button) sends a wakeup directly.
- Pull-side: same channel; after a successful push, immediately schedule a pull (other devices may have new data, and the round-trip we just made established connectivity).

**Backoff on failure:**
- Success: next tick at `30s` from now (the existing baseline cadence).
- Failure: exponential backoff with jitter — `min(30s * 2^attempts, 30min)` plus `±25%` jitter. Reset attempts on success.
- Cap at 30 minutes so a long server outage doesn't push retries to hours.

**Idle floor:**
- Even with no local activity, pull every 30s on success-path to catch other devices' changes. (No server-push channel exists; polling is the only mechanism.)

### Single-flight guarantee

`sync_with_server` must not be re-entered. A wakeup that arrives mid-sync should set a "rerun after current finishes" flag, not start a parallel sync. Otherwise the same event_log rows get pushed twice and unique-index dedup fires (correct but wasteful). A `tokio::sync::Mutex` around the sync routine handles this.

## Steps

### 1. SyncCommon protocol additions

In `SyncCommon/src/lib.rs`:

```rust
pub struct PullChangesQuery {
    pub user_id: String,
    pub library_id: String,
    pub since_server_seq: u64,
    pub limit: Option<u32>,   // NEW
}

pub struct PullChangesResponse {
    pub changes: Vec<Change>,
    pub has_more: bool,       // NEW
}
```

`PushChangesRequest` / `Response` unchanged.

### 2. Server: pagination

In `Server/src/routes.rs` pull handler:
- Read `limit` query param. Default 500, max 5000, clamp.
- Append `LIMIT $limit` to the query in `Server/src/db.rs` (find the equivalent of `SELECT ... FROM library_events WHERE server_seq > $1 ORDER BY server_seq ASC`).
- Return `has_more = true` if the query returned exactly `limit` rows; otherwise `false`. (Slight pessimism — a batch that exactly fills the page triggers one extra empty round trip — acceptable.)

Add an integration test: insert 1500 events, pull with `limit=500`, assert three pages of 500/500/500 with `has_more = true,true,false`.

### 3. Backend: pull loop

Replace `pull_events_from_server` (`Backend/src/sync.rs:299`) with a paginating loop:

```rust
async fn pull_events_from_server(&self, mut cursor: u64) -> Result<()> {
    loop {
        let resp: PullChangesResponse = self.http_client.get(...)
            .query(&[("limit", "500"), ...])
            .send().await?.error_for_status()?
            .json().await?;

        if resp.changes.is_empty() && !resp.has_more { return Ok(()); }

        let mut conn = self.db_pool.get()?;
        let tx = conn.transaction()?;
        for change in &resp.changes { db::apply_remote_change_in_tx(&tx, change)?; }
        db::drain_pending_remote_changes_in_tx(&tx)?;
        if let Some(last) = resp.changes.last() {
            if let Some(seq) = last.server_seq {
                cursor = seq;
                db::set_sync_metadata(&tx, "last_pull_server_seq", &cursor.to_string());
            }
        }
        tx.commit()?;

        if !resp.has_more { return Ok(()); }
    }
}
```

Each page commits independently. Crash mid-loop = lose the current page's parse work, never lose committed data.

### 4. Backend: adaptive sync loop

Rewrite `start_sync_loop` (`Backend/src/sync.rs:93`):

```rust
pub async fn start_sync_loop(&self) {
    let mut next_delay = Duration::from_secs(30);
    let mut attempts: u32 = 0;
    let mut wakeup = self.wakeup_rx.lock().await;  // mpsc::Receiver<()>
    let lock = Arc::new(tokio::sync::Mutex::new(()));

    loop {
        tokio::select! {
            _ = tokio::time::sleep(next_delay) => {}
            _ = wakeup.recv() => {}
        }

        let _guard = lock.lock().await;   // single-flight
        let result = self.sync_with_server().await;
        drop(_guard);

        match result {
            Ok(()) => {
                attempts = 0;
                next_delay = Duration::from_secs(30);
            }
            Err(e) => {
                log::error!("Sync failed: {}", e);
                attempts = attempts.saturating_add(1);
                let base = Duration::from_secs(30) * (1 << attempts.min(6));   // up to 30*64 = 32min
                next_delay = with_jitter(base.min(Duration::from_secs(1800)));
            }
        }
    }
}
```

Add a `wakeup_tx` field on `SyncManager` (`mpsc::Sender<()>`) exposed via a public method `request_sync()`. UI / scanner / event_log triggers route through it.

### 5. Backend: debounced trigger ping

In the path that writes to `event_log` (the trigger path), call a small helper that pings `wakeup_tx` after a 2-second debounce. Easiest implementation: a separate task holding a `tokio::time::Instant` of the last ping; on each ping, schedule a wakeup at `last_ping + 2s` (cancelling previously scheduled). Or use a `tokio::time::sleep` reset pattern.

Don't ping from inside the SQLite trigger itself (triggers can't talk to Tokio). Instead, the Rust code that performs the mutation pings after the transaction commits. Add the ping at all mutation sites: `scanner.rs` (after each scan transaction), `epub.rs` reading-position writes, etc. Or add a single `db::commit_with_notify(tx, &wakeup_tx)` helper.

### 6. UI / external wakeup hooks

Expose `SyncManager::request_sync()` to:
- A "Sync now" button in the UI.
- Library-level events that the user expects to propagate immediately (book added, book deleted).

These all just send `()` to the wakeup channel. Idempotent — multiple sends collapse into one wake-up because the receiver only consumes when sleeping.

### 7. Tests

- **Pagination correctness**: server with N events, client with `limit=K`. Pull loops, ends with `last_pull_server_seq == max(server_seq)`.
- **Crash mid-pagination**: kill client between pages; restart; assert it resumes from `last_pull_server_seq` and ends in the same final state.
- **Backoff math**: simulate 5 consecutive failures, assert delays are `~30s, ~60s, ~120s, ~240s, ~480s` ± jitter.
- **Backoff reset on success**: after backoff has expanded, a successful sync resets `attempts` to 0 and `next_delay` to 30s.
- **Single-flight**: send 10 wakeups while a sync is in progress, assert exactly one extra sync runs after the current one.
- **Debounce**: 100 rapid event_log inserts produce one push, not 100.

### 8. Cleanup
- Remove the hardcoded `Duration::from_secs(30)` interval from `Backend/src/sync.rs:94` — replaced by the adaptive logic.
- Document the protocol additions in any README / API doc.

## Risks / open questions

- **Single-flight in `sync_with_server`** vs the *asset* sync (which is also called from `sync_with_server`). Asset sync can be long; the lock would block metadata syncs for the duration. Either: (a) hold the lock only for metadata push/pull and let asset sync run unguarded (it has its own per-job claim mechanism, post asset-sync plan), or (b) split into two independently-scheduled loops — `metadata_sync_loop` (this plan's adaptive logic) and `asset_sync_loop` (its own cadence). (b) is cleaner; do that. The asset-sync plan already wants a `TransferOrchestrator` separate from metadata sync, so the split aligns naturally.

- **Server-side `limit` enforcement.** A misbehaving client sending `limit=10000000` should get clamped, not OOM the server. Clamp inside the handler before the SQL call.

- **Cursor ordering stability.** `server_seq` is `BIGSERIAL`, monotonic, no gaps that matter. Pagination by `WHERE server_seq > cursor ORDER BY server_seq ASC LIMIT N` is stable. No `OFFSET` (correctness footgun under concurrent inserts) — always use `WHERE server_seq > cursor`.

- **Has_more pessimism.** A page returning exactly `limit` rows triggers one extra empty round trip. Cheap. The alternative (peek `limit+1`) is uglier.

- **Interaction with HLC plan.** None. HLC is about ordering / conflict resolution; this is about transport. Land in any order.

- **Interaction with auth plan.** Auth will move `user_id` out of the query string (today at `Backend/src/sync.rs:303`) into a header. This plan adds a `limit` query param. Both touch the same line; coordinate, but neither blocks the other.

## Estimated effort
~1 day. Pagination is mostly mechanical (server query + client loop + protocol field). The adaptive-cadence loop is a small state machine plus mutation-site wakeup pings — wakeup plumbing is the only place that touches a lot of files. Tests take half a day if you do them properly.
