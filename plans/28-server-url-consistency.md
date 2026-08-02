# Plan 28: Server URL consistency (tiny)

`Session.server_url` (stored at login, `Backend/src/auth.rs`) and `SyncManager.server_url`
(env `SYNC_SERVER_URL` at construction, `Backend/src/library.rs::sync_server_url`) can
disagree: log in against server A, restart with the env var pointing at B → token from A is
sent to B. Harmless today (single self-hosted server) but a confusing failure when it isn't.

## Change

Pick one source of truth: the **session's** server_url wins while signed in (the token is only
valid there); the env var is the default for login/signup and signed-out display. Concretely:
`SyncManager` request paths use `session.server_url` when a session exists; `sync_server_url()`
remains the pre-login default. Log a warning at startup if the two differ. The bound-account
guard (`bound_user_id`) already prevents cross-account data mixing; this closes the
cross-server token leak/confusion.

Test: session with server A + env pointing at B → push/pull/blob requests hit A; after
logout, login flow targets B.
