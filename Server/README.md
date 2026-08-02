# Server architecture

The server is a small composition binary over four crates:

```text
                         +--------------------+
                         |    sync-server     |
                         | configuration/main |
                         +----+----+----+------+
                              |    |    |
              +---------------+    |    +----------------+
              v                    v                     v
      +---------------+    +----------------+    +--------------------+
      |  server-http  |    |server-postgres |    |server-asset-store  |
      | Axum adapter  |    | SQLx adapter   |    | filesystem adapter |
      +-------+-------+    +--------+-------+    +----------+---------+
              |                     |                       |
              +---------------------+-----------------------+
                                    v
                            +---------------+
                            |  server-core  |
                            | ports + use   |
                            | cases         |
                            +---------------+
```

- `server-core` owns application ports, synchronization authorization and
  orchestration, garbage-collection orchestration, and shared errors. It has no
  HTTP, SQL, or filesystem dependencies.
- `server-postgres` implements authentication and synchronization persistence,
  owns migrations, and provides PostgreSQL advisory-lock coordination. Pushes
  take a shared global lock; garbage collection takes its exclusive counterpart.
- `server-asset-store` implements content-addressed book and thumbnail storage.
- `server-http` translates Axum requests and responses to the core service ports.
- `sync-server` reads configuration and wires concrete adapters into the core and
  HTTP router. It is the only crate allowed to depend on all adapters.

Adapters may depend on `server-core`; `server-core` must never depend on an
adapter. Cross-adapter work belongs in a core use case, not in an HTTP handler or
database module.

## Boundary enforcement

- Every server package is `publish = false` and inherits workspace lints that
  forbid unsafe code, unreachable public items, and mismatched public interfaces.
- `server-http` exposes only its router constructor. Its Axum state, handlers,
  extractors, and wire DTOs remain crate-internal.
- `server-postgres` exposes an opaque `PostgresDatabase`; its SQLx pool, queries,
  crypto helpers, and synchronization store remain adapter-internal.
- `tests/architecture.rs` checks the Cargo dependency graph, rejects adapter-to-
  adapter dependencies and technology leakage, checks publication policy, and
  scans all `server-core` Rust sources for adapter APIs.

Run `cargo test --manifest-path Server/Cargo.toml --workspace` to execute the
architecture checks together with the server tests.

## Book quota and transfer fairness

Book blobs use a PostgreSQL quota ledger. Physical files remain globally
deduplicated by content hash, while each user is charged once for every unique
book hash referenced anywhere in that user's libraries. Metadata and thumbnails
do not consume book quota. New accounts receive a 5 GiB quota lazily; a plan or
administrative operation changes `user_storage_account.quota_bytes` for the
specific user. Quota changes go through the audited server operation:

```text
POST /api/admin/users/{user_id}/storage-quota
Authorization: Basic <bokheim-admin credentials>

{
  "change_id": "billing-event-or-admin-request-id",
  "quota_bytes": 21474836480,
  "reason": "upgraded to 20 GiB"
}
```

`change_id` is an idempotency key: retrying the identical change is safe, while
reusing it for different values returns `409 Conflict`. Every successful change
records the actor, previous and new limits, reason, and timestamp. Downgrading
below current usage is allowed; existing books remain available and new uploads
are rejected until usage returns below the new limit.

Uploads require an exact `Content-Length`. The server reserves that many bytes
before storage, validates the completed book, then atomically converts the
reservation into used bytes. Failed and abandoned reservations are released;
removing the user's final synchronized reference refunds the charge.

Upload and download traffic have independent, work-conserving deficit-round-
robin schedulers. Scheduling is by authenticated user rather than connection,
so parallel transfers cannot increase a user's share under contention. Configure
the global ceilings and burst allowance with:

```text
BOKHEIM_UPLOAD_BYTES_PER_SECOND=104857600
BOKHEIM_DOWNLOAD_BYTES_PER_SECOND=104857600
BOKHEIM_TRANSFER_BURST_BYTES=1048576
```

A direction set to `0` is deliberately unlimited. Defaults are 100 MiB/s in
each direction with a 1 MiB burst.

## Network transport

The server defaults to plain HTTP on `127.0.0.1:8080` for local development and
tests. A non-loopback TCP binding is rejected unless explicitly acknowledged
with `SYNC_ALLOW_INSECURE_HTTP=true`.

The production deployment terminates public HTTPS in Caddy and uses a
permission-restricted Unix socket for the application boundary:

```text
SYNC_SERVER_SOCKET=/run/bokheim/sync.sock
```

Unix-socket mode trusts the single sanitized `X-Forwarded-For` value supplied by
Caddy. TCP mode always ignores forwarding headers and uses the direct peer
address. Embedded certificate management is deliberately not part of the
application. See [`deploy/README.md`](deploy/README.md) for the complete public
Internet deployment.
