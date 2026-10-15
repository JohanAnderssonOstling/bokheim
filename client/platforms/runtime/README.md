# Client platform runtime

Owns native/browser execution, bounded storage scheduling, and generic coordinator
queues. It has no dependency on library persistence or application services.

`transfers` applies the device's metered-connection preference to file transfers.
It monitors Android's connectivity service, NetworkManager on Linux, Windows
network cost, and macOS expensive paths/Low Data Mode. Unknown cost is allowed.
Async transfers wait before admission and cancel on a cost change; durable queues
retain their jobs, while fetches without a queue retry from their original inputs.
The app's registry owns the setting; `transfer-policy.json` is an atomic mirror
read by the separate embedded-browser helper. User pause/seeding choices remain
independent of automatic suspension.

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
