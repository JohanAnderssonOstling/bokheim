# Web backend application

This package owns the browser application's worker topology and entry points:

- `src/lib.rs`: database worker startup and application request dispatch.
- `examples/sync_coordinator.rs`: shared coordinator entry point.
- `src/coordinator.rs` and `src/hosts.rs`: wiring backend services to an elected browser host.
- `examples/cpu_worker.rs`: CPU worker entry point.
- Worker loader `.js` files: application bootstrap and database ownership configuration.
- `connection.rs`: deployment identity and worker script URLs, shared with the web UI and worker probes.

`client-platform-web` owns worker creation, tab election, lifetime locks, ports,
CPU execution queues, transfer coordination, storage mechanisms, and network I/O.
It accepts opaque payloads and callbacks and has no dependency on the backend.

`app` owns application commands and workflows. Native and browser imports run
the library's `import_selected_directory`; the browser sends the selected files
beside the import request to the database worker. `web` translates browser
capabilities and messages into backend operations.
Sync uses the same `library-sync` on native and browser targets. Request correlation
and disconnect draining use `client-platform-runtime::pending`.

Build the complete optimized application from the workspace root:

```sh
BOKHEIM_WEB_OUTPUT_DIR=/tmp/bokheim-web ./apps/web-gpui/scripts/build.sh
```
