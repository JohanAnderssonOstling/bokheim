# Plan 33: Separate self-hosted blob service

**Status:** 💤 deferred-by-design

Split book and thumbnail transfer from the synchronization application so the
control plane and data plane can run on machines with different hardware. This
is a possible future deployment architecture, not a prerequisite for the first
Bokheim deployment.

## Wake-up condition

Implement this plan when at least one of the following becomes true:

- blob capacity or transfer bandwidth is constrained by the database/API host;
- the storage disks need to move to a different physical machine;
- asset traffic materially interferes with metadata synchronization;
- independent scaling or maintenance of storage becomes operationally useful.

Until then, the existing in-process `AssetStore` is simpler and remains the
preferred deployment.

## Decision

Add a second, self-hosted Rust application named `bokheim-blob`. It stores files
on locally attached large-capacity disks and serves their bytes directly to
clients. It does not use AWS, require S3, connect to PostgreSQL, or receive normal
Bokheim session tokens.

The synchronization server remains the control plane. It authenticates users,
checks library ownership and live content references, and issues short-lived,
signed capabilities for individual transfer operations. The blob server is the
data plane. It verifies those capabilities locally and performs only the
authorized storage operation.

```text
                             PostgreSQL
                                 ^
                                 |
Client -- metadata/auth --> bokheim-sync
   |                             |
   |                       signed capability
   |                             |
   +---- book/thumbnail bytes -->+--> bokheim-blob --> large local disks
```

Suggested public endpoints:

- `sync.bokheim.se` for authentication, libraries, synchronization, and transfer
  authorization;
- `blobs.bokheim.se` for direct upload, download, range, existence, and health
  requests.

The client must use the URL returned by the control plane. It must not construct
the blob hostname itself; this keeps storage placement changeable.

## Goals

- Keep PostgreSQL and the synchronization process on fast, modest-capacity
  storage.
- Put large immutable content on a machine optimized for disk capacity and
  network throughput.
- Ensure book bytes never pass through the synchronization server.
- Preserve the current authorization rule: a user may access a hash only when
  they own the library and its current projection references that hash.
- Preserve content-address integrity, immutable publication, byte-range
  downloads, size limits, and batch existence checks.
- Allow either service to be maintained and scaled independently.

## Non-goals

- Do not introduce AWS or another hosted storage provider.
- Do not require the S3 protocol. An S3-compatible adapter may be added later,
  behind the storage port, without changing this design.
- Do not move authentication, synchronization metadata, or library authorization
  into the blob service.
- Do not make the blob service a general-purpose file server.
- Do not implement distributed garbage collection in the first migration.

## Trust and authorization boundary

Use asymmetric signatures for transfer capabilities:

- `bokheim-sync` holds the private signing key;
- `bokheim-blob` receives only the public verification key;
- signing-key material is never stored on the blob host;
- normal session bearer tokens are never sent to the blob host.

Use Ed25519 with a versioned, canonical claim encoding. Do not sign ad-hoc JSON
whose byte representation can vary. Every capability contains at least:

- protocol version and key identifier;
- issuer `bokheim-sync` and audience `bokheim-blob`;
- operation (`get`, `head`, `put`, or batch `exists`);
- closed asset kind (`book` or `thumbnail`);
- exact content hash, or the bounded hash set for an existence request;
- upload byte limit for `put`;
- issue, not-before, and expiration timestamps.

Capabilities should normally expire after a few minutes. A range retry after
expiration obtains a new capability from the sync server. Replay during the
validity window is acceptable because GET/HEAD are read-only and PUT publishes
immutable bytes under the same content hash. Key rotation keeps the previous
public key available only for the maximum capability lifetime.

TLS remains mandatory for both public endpoints. The two services use separate
system users, credentials, writable directories, and Caddy upstreams.

## Transfer protocol

Add an authenticated control-plane endpoint similar to:

```text
POST /api/libraries/{library_id}/assets/grants
```

The request specifies operation, asset kind, and one or more content hashes. The
sync server performs the existing ownership/reference check, then returns an
expiry and opaque transfer instructions containing URL, method, capability, and
required headers. Clients treat these instructions as data and do not understand
the signature format.

Use one capability per upload or download. Use one bounded batch capability for
the existing `exists` operation so a client does not add a network round trip for
every hash. Retain the current maximum batch size.

The blob server supports:

- `HEAD` and `GET`, including the existing single open-ended byte range;
- `PUT` with streaming size enforcement;
- batch existence checks;
- immutable ETags derived from the content hash;
- a private or minimally informative health endpoint.

Large book transfers tolerate the extra authorization round trip. Small
thumbnails do not, so grants and existence checks must be batched and immutable
thumbnail responses should be cacheable.

## Storage correctness invariants

The split must preserve all of these properties:

1. A content hash identifies bytes, not a book or placement.
2. The final object path is derived only from the parsed, canonical hash and
   closed asset kind; user-controlled path components never reach the filesystem.
3. An upload is written to a private temporary file while streaming.
4. The blob server enforces the kind-specific maximum size even when
   `Content-Length` is absent or false.
5. The blob server computes the application's BLAKE3 hash itself and rejects a
   mismatch. A client-provided hash alone is not proof of integrity.
6. A verified upload is atomically published and never exposes partial bytes.
7. Existing content is immutable. Concurrent correct uploads converge on the
   same final object; an existing object is never replaced by different bytes.
8. A failed or interrupted upload leaves no published object and only a
   reclaimable temporary file.
9. Downloads preserve `ETag`, `Accept-Ranges`, `Content-Length`, and correct
   `206`/`416` behavior.
10. Denied or expired capabilities cause no storage reads, writes, or existence
    disclosure.

The existing `server-asset-store` implementation is the starting point. Extract
and rename boundaries only where necessary; do not rewrite its validated
streaming, hashing, atomic-publication, or sweep behavior.

## Crate and process boundaries

Evolve the workspace toward these responsibilities:

- a small shared asset-protocol/domain crate owns `AssetKind`, size limits,
  canonical capability claims, and transfer DTOs;
- `server-core` authorizes requested hashes and issues transfer instructions;
- `server-postgres` remains the only PostgreSQL adapter;
- `server-http` exposes control-plane grant endpoints but no final byte stream;
- a blob-core crate validates capabilities and orchestrates storage operations;
- the filesystem asset-store crate implements the blob-core storage port;
- `bokheim-sync` composes control-plane adapters;
- `bokheim-blob` composes capability verification, HTTP, and filesystem storage.

Enforce in architecture tests that:

- blob crates cannot depend on `server-postgres` or authentication persistence;
- sync/control crates cannot depend on the filesystem asset adapter after the
  migration is complete;
- only the protocol/domain crate is shared across the two application sides;
- no adapter depends on another adapter.

## Distributed garbage collection

Disable automatic and administrative blob deletion during the initial split.
The current collector coordinates PostgreSQL references and local file deletion
under one maintenance lease. Moving deletion to another process without replacing
that coordination creates a race in which a newly referenced object can be
deleted.

Retaining an unreferenced content-addressed object is a storage leak, not a
correctness failure. Deleting a live object is a correctness failure, so the
initial design deliberately chooses retention.

Add distributed GC only as a later phase. The first correct implementation is:

1. the sync server acquires the existing exclusive database maintenance lease,
   blocking pushes that can change live references;
2. it snapshots the exact live content-hash set;
3. it sends an authenticated sweep request to the blob service and waits for the
   sweep to finish;
4. the blob service ignores young objects and upload temporaries using a generous
   cutoff, and deletes only hashes absent from the snapshot;
5. the sync server releases the database lease only after success or a known-safe
   abort.

Network failure must fail closed: retain objects and release the lease without
claiming a completed sweep. Optimize long maintenance pauses only after this
correct baseline exists. This plan supersedes the earlier assumption that GC
always runs in the same server process; coordinate any implementation with plans
23–25.

## Failure behavior

- Blob service unavailable: metadata synchronization continues; new asset
  transfers retry later and local copies remain usable.
- Sync service unavailable: already-started transfers may continue until their
  capabilities expire; no new capability can be issued.
- Network interruption: downloads resume with Range and a refreshed capability;
  interrupted uploads are retried and converge by content hash.
- Duplicate/concurrent upload: at most one verified immutable object is
  published; all correct callers receive success.
- Clock skew: keep service clocks synchronized, allow a small bounded validation
  tolerance, and never extend the signed expiration.
- Blob disk full or read-only: return a storage failure without acknowledging
  publication; health/metrics expose the condition.

The blob machine remains a data-loss boundary. Large disks are not a backup.
Production use requires independent blob backup or replication and PostgreSQL
backup, plus a periodic audit that every live database hash is present.

## Migration sequence

1. **Protocol seam:** introduce shared transfer DTOs and a grant issuer while
   retaining the current in-process asset routes as the default.
2. **Blob application:** compose the existing filesystem store behind a new HTTP
   adapter and capability verifier. Add it to local integration tests.
3. **Client support:** teach clients to request grants and follow returned URLs.
   Keep the old path available as a temporary rollback mode.
4. **Dual-host testing:** exercise offline/online recovery, interrupted and ranged
   transfers, concurrent uploads, expiry, authorization denial, and either-service
   outages across two processes.
5. **Data migration:** stop asset writes, copy existing books and thumbnails to
   the storage host, verify every filename and BLAKE3 digest, then enable the blob
   base URL and signing key.
6. **Cutover:** route `blobs.bokheim.se` directly to the storage host, observe a
   soak period, then remove byte-serving routes and disk access from
   `bokheim-sync`.
7. **Later GC:** implement the maintenance-lease protocol above only when retained
   orphan storage becomes material.

Every stage before final cutover must be independently deployable and reversible.
Do not redirect existing PUT requests across hosts as the migration mechanism;
clients must explicitly use returned transfer instructions so session headers
cannot accidentally cross the trust boundary.

## Required tests

### Capability and authorization

- valid operation/hash/kind succeeds;
- wrong audience, issuer, operation, kind, or hash is rejected;
- expired, not-yet-valid, malformed, and unknown-key capabilities are rejected;
- modifying any signed claim invalidates the capability;
- a foreign-library or unreferenced hash receives no grant;
- the blob service rejects a normal Bokheim session token;
- public-key rotation accepts only the intended overlap window.

### Storage and transfer

- payload/hash mismatch and oversized streaming bodies never publish;
- missing/false `Content-Length` cannot bypass limits;
- disconnect at every upload stage leaves no partial published object;
- many concurrent uploads of the same hash converge;
- ranged resume returns exactly the original bytes and correct headers;
- existence batches neither reveal nor probe hashes outside their capability;
- temporary-file sweeping cannot delete a currently active upload.

### Cross-process behavior

- metadata sync succeeds while the blob process is offline;
- asset retry succeeds after the blob process returns;
- a client goes offline between grant issuance and transfer, refreshes the expired
  grant, and completes after reconnecting;
- sync-process failure during an active transfer does not corrupt the object;
- blob-process failure during transfer resumes or retries safely;
- slow transfers do not occupy database connections or route bytes through the
  sync process;
- a network partition during future GC retains uncertain objects and releases the
  database lease safely.

## Acceptance criteria

- Packet/traffic observation shows asset bodies flowing directly between client
  and blob host, never through `bokheim-sync`.
- `bokheim-blob` starts and serves valid transfers with PostgreSQL unreachable.
- Possession of the blob host's configuration cannot mint a valid capability.
- The sync host has no writable blob directory after final cutover.
- The blob host has no database credentials or normal session-token verifier.
- All existing synchronization convergence tests remain unchanged and passing.
- The cross-process tests above pass repeatedly under disconnection and
  concurrency stress.
- Backups and a live-hash presence audit are operational before the blob host is
  treated as the authoritative server copy.
