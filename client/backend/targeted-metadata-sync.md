# Targeted metadata notifications

`LibraryChanged` retains its library ID instead of unconditionally refreshing
the selected library. Active-library events wake its independent worker;
inactive-library events wake the sequential background worker. Checkpointed
libraries with empty outboxes can therefore receive subsequent remote edits.

Requests are keyed by server/account and library, and coalesced with generation
tokens. A pull only acknowledges the generation it started with; notifications
arriving during an exchange remain queued. Failures and selection/account
cancellation do not consume requests. Unknown-library requests remain pending
until account discovery adds the library and wakes the worker.

Browsing reconnect/Hello and Reconcile events request catch-up pulls for known
libraries. The catch-up expands into independent library requests so one failure
does not make successful libraries repeat their work on each retry. The queue
is in memory; reconnect reconciliation covers events lost across process exits.
New libraries also retain the separate missing-first-checkpoint initial-pull
behavior.

This change does not keep the notification socket open while idle reading or
suspended, alter book-file download policy, or introduce remote polling.

Validation: `cargo test --release -p bokheim-backend app:: -- --test-threads=2`
passed **112 tests**, with 2 ignored. Coverage includes notification routing,
generation/account isolation, checkpointed inactive-library pulls, failed-pull
retry, reconnect catch-up, and active-sync independence. Native release tested;
the browser runtime was not built or launched for this change.
