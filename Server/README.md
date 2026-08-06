# Server architecture

The server is a composition binary over separately enforced account,
synchronization, storage, transport, and database boundaries:

```text
                         sync-server
                      composition + config
                     /       |          \
                    v        v           v
 server-account-http server-account-mail server-http server-asset-store
                    |        /      \           |
                    v       v        v          v
          server-account-core       server-core
                    ^                   ^
                    |                   |
     server-account-postgres     server-postgres
                    \                 /
                     v               v
                       server-database
                  connection + migrations
```

- `server-account-core` owns authenticated identity and account lifecycle
  contracts. Synchronization receives only its `IdentityResolver` capability.
- `server-account-postgres` exclusively owns password hashing, invitations,
  bearer-token persistence, public registration, durable authentication rate
  limits, the account-email outbox, and account queries.
- `server-account-http` exclusively owns signup, login, atomic bearer-session
  refresh, logout, current-account, verification, password-reset, and invitation
  HTTP routes.
- `server-account-mail` delivers queued account mail through a local
  sendmail-compatible relay. It cannot access PostgreSQL or synchronization
  services.
- `server-core` owns application ports, synchronization authorization and
  orchestration, garbage-collection orchestration, and shared errors. It has no
  HTTP, SQL, or filesystem dependencies.
- `server-postgres` implements synchronization and quota persistence, atomic
  library-ownership authorization, and PostgreSQL advisory-lock coordination.
  It cannot access credential tables.
- `server-database` owns the shared PostgreSQL capability and ordered schema
  migrations used by the independently owned persistence adapters.
- `server-asset-store` implements content-addressed book and thumbnail storage.
- `server-http` translates synchronization and blob requests to core service
  ports. It receives identity resolution but no credential-management service.
- `sync-server` reads configuration and wires concrete adapters into the core and
  HTTP router. It is the only crate allowed to depend on all adapters.

Core crates never depend on adapters. Account and synchronization PostgreSQL
adapters are peers over `server-database`, never dependencies of one another.

## Boundary enforcement

- Every server package is `publish = false` and inherits workspace lints that
  forbid unsafe code, unreachable public items, and mismatched public interfaces.
- Account and synchronization HTTP crates expose only router constructors. Their
  Axum states, handlers, extractors, and wire DTOs remain crate-internal.
- Architecture tests reject credential SQL in `server-postgres` and reject sync
  or blob SQL in `server-account-postgres`.
- `tests/architecture.rs` checks the Cargo dependency graph, rejects adapter-to-
  adapter dependencies and technology leakage, checks publication policy, and
  scans all `server-core` Rust sources for adapter APIs.

Run `cargo test --manifest-path Server/Cargo.toml --workspace` to execute the
architecture checks together with the server tests.

## Public registration

Invitation signup remains available independently. Public signup is disabled by
default and is exposed only when `BOKHEIM_PUBLIC_REGISTRATION=true`. Enabling it
also requires:

- `BOKHEIM_MAIL_FROM`, a plain sender mailbox;
- `BOKHEIM_DISPOSABLE_EMAIL_DOMAINS_FILE`, an actively maintained, non-empty,
  one-domain-per-line denylist no larger than 4 MiB;
- a sendmail-compatible local queue at `/usr/sbin/sendmail`, or an alternate
  absolute path in `BOKHEIM_SENDMAIL_PATH`.

Registration reserves the canonical email and username for 24 hours and stores
only an Argon2 password hash plus a hash of the verification token. Verification
is single-use and creates a one-hour access token plus a 90-day rotating refresh
session. Password-reset tokens live for one hour, are single-use, and revoke all
sessions when consumed. Registration,
resend, verification, login, and reset routes use PostgreSQL-backed IP and
subject buckets, so limits survive restarts and work across server processes.

Requests for existing accounts and unknown password-reset emails deliberately
return the same `202 Accepted` response. Disposable-domain policy rejection and
malformed email/password inputs return `400` because they reveal no account
existence.

## Authentication lookup cache

Successful access-token resolution is cached inside the account component so a
hot synchronization exchange needs only its synchronization SQL statement. The
cache stores the SHA-256 token digest, authenticated identity, owning session,
and absolute token expiry; it never stores the raw bearer token. It is bounded
to 65,536 entries, has a maximum 30-second residency, and still rejects an entry
immediately at the token's absolute expiry.

Refresh rotation, refresh-token reuse, logout, password reset, individual
session revocation, and bulk revocation invalidate affected entries after the
database transaction commits. A cache revision closes the query/revocation race:
a lookup begun before invalidation cannot repopulate its result afterward.

The current deployment has one active synchronization-server process, so these
service-level invalidations are immediate. Before horizontally running multiple
server processes, add shared invalidation delivery (for example PostgreSQL
`LISTEN`/`NOTIFY`) or accept the bounded 30-second cross-process cache window.

## Book quota and transfer fairness

Book blobs use a PostgreSQL quota ledger. Physical files remain globally
deduplicated by content hash, while each user is charged once for every unique
book hash referenced anywhere in that user's libraries. Metadata and thumbnails
do not consume book quota. New accounts receive a 5 GiB quota lazily; a plan or
administrative operation changes `user_storage_account.quota_bytes` for the
specific user. Quota changes go through the audited server operation:

The administrative quota endpoint is
`POST /api/admin/users/{user_id}/storage-quota`. Like every typed Bokheim API
endpoint, its request and response use the versioned
`application/vnd.bokheim+bincode; version=5` wire format rather than JSON. Its
request contains `change_id`, `quota_bytes`, and `reason`; invoke it through a
Bokheim administrative client so framing and version negotiation cannot be
omitted accidentally.

`change_id` is an idempotency key: retrying the identical change is safe, while
reusing it for different values returns `409 Conflict`. Every successful change
records the actor, previous and new limits, reason, and timestamp. Downgrading
below current usage is allowed; existing books remain available and new uploads
are rejected until usage returns below the new limit.

Uploads require an exact `Content-Length`. The server reserves that many bytes
before storage, validates the completed book, then atomically converts the
reservation into used bytes. Failed and abandoned reservations are released;
removing the user's final synchronized reference refunds the charge.
Synchronized values are canonical typed PostgreSQL state, not a JSONB event
archive. Each domain table owns its complete value and synchronization metadata:
the total `VersionKey`, actor identity, and a globally ordered `server_seq`.
Pulls merge the indexed directory, book-lifecycle, physical-placement, reading-
position, annotation, bookmark, semantic-link, last-opened, and metadata tables.
Winning LWW updates redraw the global sequence so offline clients receive the
new value, while retries and losing versions do not grow storage or redraw the
cursor. A push therefore needs only one row write, with no shared envelope join.
Every synchronized wall-clock field uses the `UnixMillis` protocol type. It is
encoded as a plain integer but is explicitly measured in Unix milliseconds and
bounded to the signed 64-bit range shared by PostgreSQL and SQLite. The server
rejects LWW clocks beyond the allowed future-skew window.
Per-library reading and general-state revision heads are maintained by
transition-table statement triggers. A scanner batch advances each head once
per non-empty typed-state statement rather than rewriting the same head row for
every book property in that statement.

A newly scanned book publishes two state cells: lifecycle and physical
placement. Lifecycle carries the title and ordered author credits as initializer metadata, avoiding
a redundant metadata mutation for every ingest. It only fills a client row that
does not have metadata yet. Subsequent title or author-list edits—including changes
made while reactivating an existing book—continue to use the independent
metadata register, so those edits cannot accidentally change deletion or
resurrection ordering.

Physical placement is a path register: additions compete for a directory/name
path and removals affect only its current occupant. The canonical placement
upsert returns its winning old/new content hashes directly, so per-user blob
reference counts are updated without a second path projection. Ordinary state
updates such as reading positions do not touch placement or blob references.

Every typed account, library, preference, asset-manifest, administration, and
synchronization body negotiates the versioned
`application/vnd.bokheim+bincode; version=5` representation. The wire body
starts with a Bokheim magic/version header and uses positional bincode fields
and numeric enum variants. Bodies of at least 1 KiB are zstd level-1 compressed when compression makes them
smaller. Decoded bodies retain the 8 MiB request limit, preventing compressed
expansion from bypassing request bounds. JSON is neither accepted nor returned
by this protocol; unsupported media types, missing version negotiation, and
unsupported versions fail before application logic runs. Raw book and thumbnail
endpoints continue to transfer their exact validated bytes.
Push and exchange pages are bounded to 1,000 mutations and clients also stop at
the 4 MiB encoded-mutation target, whichever limit is reached first.
Pull pages may cut across directory dependencies. SQLite clients therefore
materialize unresolved directories as hidden tombstoned placeholders and
activate the hierarchy only when its real parent state arrives. A child or book
placement can consequently commit on one page without violating foreign keys or
becoming visible before a dependency delivered by a later page.

Compression is deliberately limited to synchronization metadata. Book and
thumbnail routes retain their original bytes, content hashes, content lengths,
and byte-range semantics and never pass through the sync codec.

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

## Operational concurrency

The PostgreSQL pool and CPU-heavy book validation have separate bounded
controls:

```text
BOKHEIM_DATABASE_MAX_CONNECTIONS=32
BOKHEIM_DATABASE_MIN_CONNECTIONS=2
BOKHEIM_DATABASE_ACQUIRE_TIMEOUT_SECONDS=5
BOKHEIM_DATABASE_IDLE_TIMEOUT_SECONDS=600
BOKHEIM_MAX_CONCURRENT_BOOK_VALIDATIONS=2
BOKHEIM_MAX_STAGED_BOOK_UPLOADS=16
BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT=4
BOKHEIM_MAX_STAGED_BOOK_BYTES=4294967296
BOKHEIM_MAX_STAGED_BOOK_BYTES_PER_ACCOUNT=2147483648
BOKHEIM_MAX_IN_FLIGHT_REQUESTS=512
BOKHEIM_NOTIFICATION_HEARTBEAT_SECS=60
BOKHEIM_NOTIFICATION_TIMEOUT_SECS=150
BOKHEIM_MAX_NOTIFICATION_CONNECTIONS=100000
BOKHEIM_MAX_NOTIFICATION_CONNECTIONS_PER_ACCOUNT=16
```

Database maximum/minimum connections accept `2..=256` and `0..=max`
respectively. Acquire timeout accepts 1–60 seconds, idle timeout accepts 30
seconds–24 hours, and concurrent validation accepts 1–32. Staged uploads are
bounded both by count and declared bytes before temporary files are created;
the defaults allow 16 files/4 GiB globally and 4 files/2 GiB per account.
Capacity exhaustion returns HTTP 429 before the asset store reads the body.
Defaults are intended
for the current single-process deployment; raise them only after measuring
PostgreSQL, CPU, memory, and disk saturation together.
The HTTP ceiling includes requests executing handlers and requests waiting for
downstream capacity. Its default of 512 absorbs short reconnect bursts, while
the outer request deadline bounds time spent waiting. It does not increase
steady-state database throughput.

## Asset storage configuration

Local development stores assets in `./books` and `./thumbnails`. Production
deployments must configure both storage roots together as distinct,
non-nested absolute paths:

```text
BOKHEIM_BOOKS_DIR=/var/lib/bokheim/assets/books
BOKHEIM_THUMBNAILS_DIR=/var/lib/bokheim/assets/thumbnails
```

Configuring only one path, using a relative or non-normalized path, or nesting
one root under the other is a startup error. Before accepting traffic, the
server creates both roots and verifies that each supports create, write, sync,
and delete operations. See [`deploy/README.md`](deploy/README.md) for the bind
mount that safely exposes `/home/johan/data/bokheim` through the systemd home
sandbox.

Clients upload covers separately so a device can browse books without first
downloading them. Covers are private account data rather than globally shared
blobs: storage paths and garbage-collection liveness include the authenticated
user. The server streams the client's original bytes without image decoding or
re-encoding or format inspection, while enforcing a 4 MiB transfer limit.
Responses use `image/png`, `X-Content-Type-Options: nosniff`, and private
immutable caching. Clients validate and decode within bounded image limits
before display. Covers from the former global layouts remain readable and are
copied unchanged into each user's scope on first access, so an upgrade does not
blank existing cloud covers.

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

## Local server stress test

`tests/run_server_stress.sh` provisions a throwaway PostgreSQL cluster, starts
the real HTTP server on loopback, and exercises concurrent synchronization
through authentication, HTTP, PostgreSQL, and response decoding. It compares
independent-library writes with intentionally serialized same-library writes,
coalesced reading-position register updates, complete single-request position
exchanges, concurrent full pulls, and concurrent validated EPUB transfers.
It verifies that every acknowledged event is observable and every downloaded
blob exactly matches its upload:

```sh
Server/tests/run_server_stress.sh
```

The harness reports throughput and p50/p95/p99/max request latency. Workload
size can be changed with `BOKHEIM_STRESS_WORKERS`,
`BOKHEIM_STRESS_REQUESTS_PER_WORKER`, and
`BOKHEIM_STRESS_EVENTS_PER_REQUEST`. Validated EPUB payload size is controlled
by `BOKHEIM_STRESS_BLOB_BYTES`. All values are bounded, and the workload binary
refuses non-loopback URLs so it cannot accidentally load-test the public service.

`BOKHEIM_STRESS_MODE=ingest` uses the production bincode+zstd sync transport.
