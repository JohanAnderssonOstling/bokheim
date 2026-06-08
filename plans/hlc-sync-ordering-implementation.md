# HLC sync ordering implementation report

## Summary
The sync conflict-ordering model was moved from wall-clock plus ad-hoc tie-breaks to a Hybrid Logical Clock (HLC) tuple model.

This was done to remove deterministic clock-skew failures and replace ambiguous cross-client tie-break behavior with a total ordering tuple:
- `(hlc.pt, hlc.l, node)`
- where `node` is `client_id`.

## Why this was done
The previous singleton replacement logic depended on:
- `changed_at` (client wall clock),
- then `local_seq`,
- then lexicographic `client_id`.

That had two systemic issues:
1. Device clock skew could consistently force wrong winners.
2. `local_seq` is not meaningful across devices.

There was also special-case payload enrichment (`session_epoch` / `session_seq`) for one event type, which indicated missing general ordering semantics.

HLC addresses this by making event order explicit and monotonic across local emission and remote observation.

## Implemented changes

### 1) Shared protocol (`SyncCommon`)
- Added `Hlc` type:
  - `pt: u64`
  - `l: u64`
- Added `hlc: Hlc` to `Change`.
- Added HLC clock advancement helpers:
  - `tick_local`
  - `tick_recv`
- Replaced singleton replacement logic to compare only HLC tuple + node.
- Removed session-epoch/session-seq enrichment path.

### 2) Backend local storage + emit path (`Backend`)
- Extended `event_log` with:
  - `hlc_pt`
  - `hlc_l`
- Added trigger-level HLC assignment via `trig_event_log_hlc`:
  - computes next local HLC,
  - writes HLC to inserted event row,
  - persists same HLC back to `sync_metadata`.
- Added trigger guards on all event-producing triggers:
  - local-origin only (`change_origin == local`),
  - first-sync gate (`first_sync_done == 1`).
- `get_unpushed_events` now emits HLC from `event_log`.
- Remote apply path now observes incoming HLC and advances local HLC using `tick_recv`.

### 3) Backend startup policy
- Local DB is reset/recreated on every startup.

Rationale:
- Avoided local schema migration complexity while implementing HLC.
- Matches explicit project direction for current phase.

### 4) Sync first-contact gate (`Backend/sync`)
- `first_sync_done` initialized to `0`.
- Flipped to `1` after successful push response and after successful pull response.

Rationale:
- Prevent low-quality pre-sync clocks from emitting ordering-critical events.

### 5) Server persistence + ordering (`Server`)
- Added server columns on `library_events`:
  - `hlc_pt BIGINT`
  - `hlc_l BIGINT`
  - `hlc_node TEXT`
- Singleton writes now use atomic upsert with HLC tuple comparison in `WHERE` clause.
- Non-singleton dedup now uses atomic Postgres conflict handling with predicate-qualified conflict target:
  - `ON CONFLICT (library_id, client_id, client_seq) WHERE event_type NOT IN (...) DO NOTHING`
- Pull path now returns HLC to clients.

### 6) Server clock plausibility guard
- Added push-time reject for implausible future HLC:
  - request is rejected if any `change.hlc.pt > server_now + 1 hour`.

Rationale:
- Prevent corrupted/mis-set client RTC from poisoning ordering.

## Design outcomes
- Singleton conflict replacement is now atomic and HLC-driven.
- Non-singleton dedup remains atomic with conflict-target predicate inference.
- Client payload no longer depends on special-case session epoch tie-break metadata.
- Ordering semantics are explicit and shared across client/backend/server.

## Remaining work (not part of this implementation pass)
- Add dedicated invariant tests for:
  - per-row tick behavior,
  - trigger guard enforcement,
  - monotonic local HLC progression,
  - skewed-clock multi-device integration behavior.
