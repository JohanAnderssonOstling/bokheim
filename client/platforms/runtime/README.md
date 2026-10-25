# Client platform runtime

Owns native/browser execution, bounded storage scheduling, and generic coordinator
queues. It has no dependency on library persistence or application services.

`transfers` applies the device's metered-connection preference to automatic file
transfers. It monitors Android's connectivity service, NetworkManager on Linux,
Windows network cost, and macOS expensive paths/Low Data Mode. Unknown cost is
allowed. Automatic transfers cancel on a cost change; durable queues retain
jobs, while fetches without a queue retry from their original inputs. Queue
workers use `try_run` to defer blocked work without occupying the worker.
Opening and reading books (including remote reads and audiobook streaming),
explicit downloads, cross-library copies and approved app updates bypass this
restriction. Queued book downloads persist their `UserInitiated` origin so
retries and restarts preserve the exemption. Pending torrents are explicit
downloads; their completed seeding remains automatic. User pause/seeding choices
remain independent of automatic suspension. The app's registry owns the setting;
`transfer-policy.json` remains its atomic mirror for native workers.

`coordinator` owns bounded payload queues:

- `SyncJobs` coalesces requests by library and preserves every reply recipient.
- `TransferJobs` shares execution among subscribers and retains cancelled owners
  until execution finishes.
- `CpuJobs` serializes CPU work and returns owned jobs when a worker fails.

Browser adapters supply message ports and responses. Application shutdown order
remains in `web-backend-worker`'s `examples/sync_coordinator/coordinator.rs` because it names the
application's workers and controls their teardown.

```sh
cargo test --release -p client-platform-runtime --lib
```
