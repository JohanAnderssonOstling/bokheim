# Plan 19: Token expiry & 401 handling

Atomic; touches `Backend/src/auth.rs`, `Backend/src/sync.rs`, server auth responses.

## Problem

The server expires tokens (`Server/migrations/0008_token_expiry.sql`) but the client neither
stores expiry nor reacts to 401: after expiry every sync fails with a logged error, forever,
with no signal to the user. Auth is silently dead until a manual re-login.

## Change

1. Server: include `expires_at` in `AuthResponse`; ensure expired/invalid tokens yield 401
   (not 500) on all authed routes.
2. Client `Session`: store `expires_at`; `SyncManager` skips sync attempts with an expired
   token exactly as it does when signed out (and proactively treats "expires within a day" as
   expired to avoid mid-sync death).
3. 401 handling: any 401 from push/pull/blob routes → clear the in-memory session (keep
   `bound_user_id`), emit the auth-expired event (plan 22's `SyncStatus` — until that lands,
   log at error). Do **not** delete `session.json` automatically beyond marking it invalid;
   re-login flow already exists (`login_blocking` + `adopt_session`).
4. Optional follow-up (separate decision): refresh tokens / re-issue on activity. Not in this
   plan — expiry visibility first.

## Tests

1. Expired `expires_at` → `sync_once` is a no-op (no HTTP), like signed-out mode.
2. Mocked 401 on pull → session cleared, subsequent sync no-ops, `adopt_session` after
   re-login restores sync. Binding guard still rejects a different account.
