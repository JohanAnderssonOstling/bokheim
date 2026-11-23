# Client platform runtime

Owns native/browser execution, typed event-loop calls, and bounded storage
scheduling. It has no dependency on library persistence or application services.
Automatic transfers are allowed on metered connections; there is no network-cost
monitor or transfer-policy preference.

`executor::ServiceHandle` moves owned Rust arguments to an event-loop owner
and returns typed results through Tokio oneshot channels. The future is created
on its owner, allowing browser resources to remain local. Accepted operations
retain the host until they finish, including after a window closes.

The synchronous parser keeps its bounded CPU queue. Library state uses an
async read/write lock; background synchronization reacquires it between pages.
There is no application command codec, shared coordinator, or tab election.

Browser startup admits one tab with a Web Lock. The UI and storage worker use
the same Wasm module and memory. Synchronous parser/PDFium work remains in a
separate worker so it cannot stall the UI or storage event loop.
