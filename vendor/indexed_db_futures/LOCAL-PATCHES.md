# Local IndexedDB patch

Upstream: crates.io `indexed_db_futures` 0.6.4, MIT.

The transaction, request, and database-open listeners use
`Closure::once_aborting` and `Closure::wrap_aborting` at their JavaScript
boundaries. The callbacks capture notification state and generic upgrade callbacks without an `UnwindSafe`
contract. This preserves the former abort behavior if an uncaught panic escapes
an IndexedDB callback while allowing Rust operations elsewhere to use
`panic=unwind`. It does not assert that an interrupted upgrade or transaction
callback is safe to resume.

Remove the workspace's `[patch.crates-io]` override when upstream provides a
compatible callback policy.
