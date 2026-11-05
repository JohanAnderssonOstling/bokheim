# Client platform runtime

Owns native/browser execution, typed event-loop calls, and bounded storage
scheduling. It has no dependency on library persistence or application services.

`transfers` applies the device's metered-connection preference to automatic file
transfers. It monitors Android's connectivity service, NetworkManager on Linux,
Windows network cost, and macOS expensive paths/Low Data Mode. Unknown cost is
allowed. Automatic transfers cancel on a cost change; durable queues retain
jobs, while fetches without a queue retry from their original inputs. Queue
workers use `try_run` to defer blocked work without occupying the worker.
Opening and reading books (including remote reads and audiobook streaming),
explicit downloads, cross-library copies and approved app updates bypass this
restriction. Queued book downloads persist their `UserInitiated` origin so
retries and restarts preserve the exemption. The app's registry owns the setting;
`transfer-policy.json` remains its atomic mirror for native workers.

The desktop application and embedded browser enable `linux-network-cost` for
NetworkManager monitoring. Kobo omits that feature and its D-Bus dependency;
it does not expose the metered-transfer setting or write the preference mirror.
Kobo transfers remain unrestricted by network cost.

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
