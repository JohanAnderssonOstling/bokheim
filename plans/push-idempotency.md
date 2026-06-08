# Push idempotency

## Goal
Make `POST /api/sync/push` exactly-once *per change* under arbitrary client retries (timeouts, dropped responses, partial-failure replays). A retried batch must never produce duplicate `server_seq` rows, and the response's `last_server_seq` must be a usable watermark even when every change in the batch was a duplicate.

## Non-goals
- No HLC clamping policy changes (already in place at `Server/src/routes.rs:69-81`).
- No conflict-resolution rule changes for singletons or set-typed entities — those are covered by other plans.
- No authentication / `client_id` spoofing defence. Out of scope.
- No streaming or chunked push protocol. Single-request batch stays.

## Background

### What exists today

Server-side push (`Server/src/db.rs:43-128`):

- **Non-singleton path** uses `ON CONFLICT (library_id, client_id, client_seq) DO NOTHING RETURNING server_seq` (`db.rs:102-105`). On conflict no row is returned, `fetch_optional → unwrap_or(0)` makes the per-change `seq = 0`.
- **Singleton path** uses `ON CONFLICT (library_id, entity_id, event_type) DO UPDATE ... WHERE EXCLUDED.hlc > existing.hlc RETURNING server_seq` (`db.rs:67-79`). When the WHERE clause rejects the update, no row is returned; same `unwrap_or(0)` fallback.
- Both paths fold per-change results via `max_server_seq = max_server_seq.max(seq)` (`db.rs:124`), then return that as `PushChangesResponse.last_server_seq`.

Client-side push (`Backend/src/sync.rs:267-298`):

- Computes `max_local_seq = changes.iter().filter_map(|e| e.local_seq).max().unwrap_or(0)`.
- POSTs the batch.
- On success, calls `db::mark_events_pushed(&conn, max_local_seq, push_response.last_server_seq)`.

### Concrete bugs and gaps

1. **`local_seq.unwrap_or(0)` collapses dedup on missing `local_seq`** (`db.rs:53`). Any change that arrives without a `local_seq` is keyed at `client_seq = 0` for dedup. If two such changes arrive in one batch, the second silently loses to `DO NOTHING`. The current emit path always sets `local_seq`, but the contract is implicit and one missing field on a future entity type is enough to silently drop events.
2. **Duplicate-batch retry returns misleading `last_server_seq = 0`.** If a client times out, retries, and the original push actually succeeded server-side, every change in the retry hits `ON CONFLICT DO NOTHING` and returns no `server_seq`. `max_server_seq` stays at 0. The client gets `last_server_seq: 0` even though the changes are durably stored. The client's `mark_events_pushed` is then called with `0`, which may or may not advance state correctly depending on its implementation. At best the watermark stays correct because `max_local_seq` is independent; at worst, future logic that consumes `last_server_seq` from a push response sees `0` and misbehaves.
3. **Singleton retry returns misleading `last_server_seq = 0` similarly.** A retried singleton with HLC equal-or-lower than the stored one is correctly rejected by the WHERE clause but returns `0`. Same misleading-watermark issue as (2).
4. **Singletons have no `(client_id, client_seq)` dedup at all.** Today this is benign because HLC monotonicity per client guarantees the WHERE clause is the dedup. But the rule isn't documented and a future bug that resets HLC on a client (reinstall, DB rebuild) could let an old `local_seq` reappear with a fresh HLC and re-supersede a strictly-newer state. Defence in depth wants `(client_id, client_seq)` dedup on singletons too, with the WHERE-on-HLC layered on top.
5. **No covering unique index documented.** The `ON CONFLICT (library_id, client_id, client_seq)` clause requires a unique index/constraint exists. Verify and pin it down — if it's only enforced by the conflict target without a constraint, Postgres will error at runtime.
6. **Per-change roundtrip inside the batch.** `insert_changes` loops one SQL statement per change. Functionally fine inside one transaction, but means a 100-change batch is 100 roundtrips. Not a correctness issue; flagged so the dedup change doesn't lock in worse latency.

## Design

### Make `local_seq` mandatory

Treat `local_seq: 0` and missing `local_seq` as malformed. `Change.local_seq` stays `Option<u64>` on the wire for backwards compatibility, but on push:

- The server validates: every change in a batch has `Some(local_seq)` with `local_seq > 0`. Reject the whole batch with `400` and a body explaining which change index is malformed. Do not accept partial batches when validation fails.
- Two changes in the same batch with the same `(client_id, local_seq)` is a client bug; reject the batch with `400`.

This kills (1) and forces every emit path to set the field. The existing trigger-driven emit path already does, so no behavioural regression.

### Look up `server_seq` on dedup hit, don't return 0

For the non-singleton path: when `INSERT ... ON CONFLICT DO NOTHING` returns no row, the row already exists. Fetch its `server_seq`:

```sql
WITH ins AS (
    INSERT INTO library_events (...) VALUES (...)
    ON CONFLICT (library_id, client_id, client_seq) DO NOTHING
    RETURNING server_seq
)
SELECT server_seq FROM ins
UNION ALL
SELECT server_seq FROM library_events
 WHERE library_id = $1 AND client_id = $7 AND client_seq = $8
LIMIT 1;
```

This always returns exactly one `server_seq` — either the newly inserted one or the existing one. The per-change result becomes truthful, and `max_server_seq` is a usable watermark even on full-batch retry.

For the singleton path: when the upsert's WHERE clause rejects the update, no row is returned, but a row *does* exist (else it would have been an insert, not an update). Same pattern:

```sql
WITH ins AS (
    INSERT ... ON CONFLICT (library_id, entity_id, event_type)
    DO UPDATE SET ... WHERE (EXCLUDED.hlc_pt, EXCLUDED.hlc_l, EXCLUDED.hlc_node)
                          > (library_events.hlc_pt, library_events.hlc_l, library_events.hlc_node)
    RETURNING server_seq
)
SELECT server_seq FROM ins
UNION ALL
SELECT server_seq FROM library_events
 WHERE library_id = $1 AND entity_id = $4 AND event_type = $3
LIMIT 1;
```

Either the insert/update happened (returned), or the existing row's `server_seq` is returned. Never 0.

This kills (2) and (3).

### Layer `(client_id, client_seq)` dedup onto singletons

Add a partial unique index for singleton events too:

```sql
CREATE UNIQUE INDEX library_events_singleton_dedup
ON library_events (library_id, client_id, client_seq)
WHERE event_type IN ('reading_position_updated', 'book_opened');
```

Then change the singleton insert to first attempt `ON CONFLICT (library_id, client_id, client_seq) DO NOTHING`. If that conflicts (true retry — same client_id, same client_seq), look up the existing `server_seq` and return it. If it does *not* conflict (genuinely new emit from this client, even if HLC is older), fall back to the existing HLC-gated upsert on `(library_id, entity_id, event_type)`.

This is a two-step write per singleton in the worst case, but singletons are rare (one per book-open / position-update). Worth the defence-in-depth from (4).

### Pin down the unique constraint

In `Server` schema, ensure:

```sql
CREATE UNIQUE INDEX library_events_client_dedup
ON library_events (library_id, client_id, client_seq)
WHERE event_type NOT IN ('reading_position_updated', 'book_opened');
```

is a real index, not just a `WHERE` clause on the `ON CONFLICT` target. Audit migrations to confirm. Add the singleton variant from the previous section. Both are partial indexes so they don't conflict.

### Client-side: trust the response, drop the `0` special case

`Backend/src/sync.rs:295` calls `mark_events_pushed(conn, max_local_seq, push_response.last_server_seq)`. After the server change, `last_server_seq` is always a real `server_seq` for some change in the batch (or zero only if the batch was empty, which is short-circuited at line 273). No client-side guard needed. Document the contract in `SyncCommon`:

> `PushChangesResponse.last_server_seq` is the maximum `server_seq` among the rows the server has durably stored for this batch's changes — including changes that were already stored from a prior attempt of the same `(client_id, local_seq)`. Zero is returned only for empty batches.

## Steps

### 1. Schema migration
- Add `library_events_client_dedup` partial unique index for non-singleton events.
- Add `library_events_singleton_dedup` partial unique index for singleton events.
- Migration plan: both indexes are `CREATE UNIQUE INDEX ... WHERE ...`. If existing data has duplicates (it shouldn't — the current `ON CONFLICT` already enforces it logically), the index creation will fail. Run a pre-flight `SELECT` to detect any duplicates and fix them before the migration.

### 2. Server: `insert_changes` rewrite
In `Server/src/db.rs::insert_changes`:
- Add a validation pass at the top: reject batches with missing-or-zero `local_seq`, or duplicate `(client_id, local_seq)` within the batch. Return a typed error mapped to `400`.
- Replace both inserts with the CTE-plus-fallback pattern from Design. Per-change result is always `Some(server_seq)`.
- Singleton path becomes two-step: `ON CONFLICT (client_id, client_seq) DO NOTHING` first, then fall back to the HLC-gated upsert if no conflict.

### 3. Server: route handler
In `Server/src/routes.rs::push_events`:
- Map the new validation error to `StatusCode::BAD_REQUEST` with a short body identifying the bad change index.
- No other changes — `last_server_seq` semantics are now contract-conformant by virtue of (2).

### 4. SyncCommon: contract docs
In `SyncCommon/src/lib.rs`:
- Doc comment on `Change.local_seq`: required on push, must be `> 0`, must be unique per `(client_id, local_seq)` for the lifetime of that client.
- Doc comment on `PushChangesResponse.last_server_seq`: as described in Design.

### 5. Client: assertion
In `Backend/src/sync.rs::push_local_events`:
- Before posting, assert every change has `Some(local_seq)` with value `> 0`. Panic in debug, log+drop the malformed change in release. (Belt and braces — server rejects too, but the local DB has already accepted it, so the right answer is to fix the emit path, not silently drop.)
- Optionally assert no duplicate `(local_seq)` in the batch — same rationale.

### 6. Tests
- **Idempotent retry, full batch.** Push batch B once → `last_server_seq = S1`. Push the identical batch B again → `last_server_seq = S1`, no new rows. Verify exactly `len(B)` rows present.
- **Idempotent retry, partial overlap.** Push batch `[A, B]` (success, `S1`). Push batch `[B, C]` → `B` dedups, `C` inserts, `last_server_seq` covers `C`. Verify three rows total.
- **Singleton retry returns existing `server_seq`.** Push singleton `S` at HLC=10 (success, `S1`). Push same singleton at HLC=5 → no update, response returns `S1` (or higher if other changes in the batch). Verify the stored row still has HLC=10.
- **Validation: missing `local_seq` → 400.** Send a change with `local_seq: None`. Assert 400 and no rows inserted.
- **Validation: duplicate `(client_id, local_seq)` in batch → 400.** Two changes with the same `local_seq`. Assert 400, no rows inserted.
- **Index existence.** Query `pg_indexes` in test setup, assert both partial indexes exist with the expected predicate.
- **Concurrent push of identical batches.** Two concurrent retries of the same batch from the same client. Both see the unique index; one wins each insert; both responses return the same `last_server_seq`. No duplicates.

### 7. Cleanup
- Remove any defensive `unwrap_or(0)` paths that became unreachable.
- If/when an audit shows the inline `ON CONFLICT` predicate (`WHERE event_type NOT IN ...`) is no longer needed because the partial index does the job, simplify the `INSERT` SQL.

## Decisions

- **`local_seq` is mandatory on push.** Wire type stays `Option<u64>` for forward compatibility, but the server enforces `Some(_) > 0`. Cleaner than changing the type and breaking deserialization for older payloads.
- **Singleton dedup is layered, not replacing HLC LWW.** `(client_id, client_seq)` catches retries; HLC compare catches semantic conflicts. Both are needed.
- **Return existing `server_seq` on dedup, not 0.** The cost is one extra SELECT per dedupped change, only on retry. Worth it for a coherent watermark.
- **Reject the whole batch on validation failure.** Partial accepts make client retry logic harder. All-or-nothing is simpler and the validation rules are cheap to satisfy.

## Risks

- **Schema migration on existing duplicate rows.** Unlikely given the existing `ON CONFLICT` logic, but possible if any historical insert path bypassed it. Pre-flight SELECT before adding the unique index.
- **Latency on retried full-duplicate batches.** Each change becomes one INSERT + one SELECT in the worst case. For a 100-change retry, that's still one transaction, two roundtrips per change. Acceptable for a rare path. If it ever shows up in profiles, batch the existence-lookup separately.
- **Interaction with HLC plan.** None. This is a transport-layer concern; HLC ordering is unchanged.
- **Interaction with set-membership plan.** None at the server layer. The set-membership conflict gate runs on the *apply* side (client), not on push. Push idempotency is orthogonal.

## Estimated effort
~1 day. Schema + migration audit (~couple hours), `insert_changes` rewrite (~half day), route validation + tests (~half day), docs (~hour).

Independent of all other plans on disk. Land it whenever convenient — it has no preconditions and unblocks nothing else, but it removes a whole class of "ghost duplicate" bugs from showing up later.
