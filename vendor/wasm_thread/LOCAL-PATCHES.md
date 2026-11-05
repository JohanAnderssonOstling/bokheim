# Local wasm_thread patch

Upstream: `zed-industries/wasm_thread` at commit
`0cf96c7708dfb97ccf3da50347e25edcf75d6937`, Apache-2.0 OR MIT.

The browser thread-management message listener uses `Closure::wrap_aborting`
instead of `Closure::wrap`. This callback owns thread lifecycle state without
an `UnwindSafe` contract. Its previous abort behavior is preserved if a panic
escapes it. The existing `catch_unwind` around a thread's user function can now
run with `panic=unwind` and return the panic through `JoinHandle` normally.

Remove the Git-source patch in the root manifest once upstream provides an
equivalent callback policy.

`Builder::spawn_async` keeps a shared-memory worker alive while an event-loop
future runs. Its `Send` factory creates the future on the destination worker;
the future and its JavaScript values never cross threads. Both JavaScript
loaders await the Rust entry point before closing the worker. Synchronous
threads retain their existing join and panic handling. An async root panic
discards its future and rejects the entry promise; the loader closes the worker
after either completion or failure.
