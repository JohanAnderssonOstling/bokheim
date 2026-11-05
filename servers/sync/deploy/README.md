# Public Internet deployment

## Curated dictionary files

Web dictionary downloads are public static files served by Caddy at
`https://app.bokheim.se/dictionaries/<sha256>.zip`; they do not use the sync API.
Before deploying a web release that adds dictionary editions, publish the
catalog's archives on the deployment host from the matching source checkout:

```sh
python3 scripts/dictionaries/publish.py \
  --output /srv/bokheim-updates/public/dictionaries
python3 scripts/dictionaries/publish.py \
  --output /srv/bokheim-updates/public/dictionaries --verify-only
```

Run as an account with write access to that persistent directory. Give Caddy
read access to the files and traversal access to their parent directories. Keep
this directory outside the rotating web release bundles. No API cache directory
or dictionary environment variable is needed.

The publisher reads `shared/dictionary-catalog/data/curated.json`, checks exact
size and SHA-256 before publishing each file atomically, and preserves the
original archive and its license/attribution notices. Repeated runs reverify and
skip existing files. Corrupt existing files cause an error; investigate and
remove that file before retrying. Interrupted downloads are not published.
Older archives are retained so released clients can still download their pinned
editions. `--id ID` can limit a diagnostic run; publish and verify the full
catalog before a web release.

Install the updated Caddyfile with the existing helper:

```sh
sudo servers/sync/deploy/install-caddy-config.sh
```

The `/dictionaries/*` handler precedes the SPA fallback, so absent files return
404 rather than HTML. Archive URLs are immutable and allow cross-origin Fetch
for development clients; production downloads use the web app's own origin.
The client still verifies every download before installing it into browser
storage. Dictionary downloads always require an explicit action in Settings.

## Reuse hosted catalog books

After deploying a server with catalog reuse support, opt in with:

```sh
sudo bash servers/sync/deploy/provision-catalog-storage.sh
sudo systemctl restart bokheim-sync.service
```

The setup uses `/srv/bokheim-updates/public/catalog/books` as the trusted public
source and `/srv/bokheim-updates/catalog-sync-pins` as a private retention
directory. The existing sync assets can stay on their separate large-disk
mount: hard links are made entirely on the catalog filesystem, so no book bytes
are duplicated or moved. The pin directory is outside Caddy's public root and
has mode 0700. Never expose it, or the private sync blob directory, through a
public file-server route.

The script grants the sync service the source-inode access Linux requires for
protected hard links, while keeping catalog directory access read-only. Its
default ACL covers normally created future catalog files. A publisher that
resets file permissions/ACLs must preserve the `bokheim` ACL or run the setup
again after publication. The service never modifies catalog book contents.

The setup writes `/srv/bokheim-updates/sync-catalog.env`, which the production
unit reads optionally. This file survives ordinary server deployments. The unit
makes the common catalog ancestor one writable mount in its otherwise read-only
namespace. Do not replace it with separate bind mounts for the book and pin
directories: `link` rejects links across mount boundaries, even when both mounts
have the same backing device. Startup probes a hosted file's hard-link
permissions and fails clearly if the storage or sandbox setup prevents reuse.

No client or database schema upgrade is needed for catalog reuse. The server
returns the existing manifest `owned` result after atomically claiming and
charging the verified public file. Existing uploaded duplicates are consolidated
by the background asset collector. Per-account quotas and private-book access
checks remain in force.

Sync pull pages use a decoded-byte budget of 63.5 MiB, with 512 KiB reserved
below the 64 MiB response ceiling. There is no independent pull change-count
cap. Native and web clients share this budget and cursor validation.

The Caddy configuration enables Zstandard/gzip for protobuf responses on the
API host, browser API proxy, and metadata API. Its explicit media-type matcher
includes `application/vnd.bokheim+protobuf`, which is outside the
[default compression matcher](https://caddyserver.com/docs/caddyfile/directives/encode).
The transferred size depends on the negotiated encoding; 4 MiB is not a strict
compressed-page limit. Install the updated Caddy configuration as well as the
coordinated client/server release to enable compression in production.

## Remote code deployment

After the host has been prepared once with `install-server.sh`, deploy a new
release from this checkout with:

```sh
servers/sync/deploy/deploy-remote.sh
```

If remote sudo must be entered from a shell on the server, stage the release
without attempting installation:

```sh
servers/sync/deploy/deploy-remote.sh --stage-only
```

The command prints the exact path of a self-contained privileged entry point.
SSH to the deployment host and run that command, for example:

```sh
sudo /tmp/bokheim-deploy.ABC123/deploy-sudo.sh
```

`deploy-sudo.sh` takes no arguments. It validates the staged artifacts and
credentials and the installed schema before stopping services.

The script builds `sync-server` and the separate `library-cloud-upgrade`
maintenance tool, transfers their artifacts and the systemd unit over SSH,
and invokes the fixed root-owned sync activation helper. It validates every
stored client payload before stopping the service, then takes a full PostgreSQL
backup and upgrades schema 14 to 17 in one transaction. Existing payloads and
retired storage configuration are retained in `bokheim_upgrade_archive`;
legacy feed records remain in `opds_feeds`. External-storage configuration is
retained in the archive while its library uses the current local storage policy.
Authored versions, asset references, accounts, and libraries are preserved.
The synchronization generation changes so clients fetch the converted values.

During activation Caddy cannot reach the candidate socket until private
readiness and protocol checks pass. Failure before publication restores the
previous database and executables. Backups remain under
`/var/backups/bokheim-sync/`. Once public traffic is admitted, deployment never
automatically restores an older database over newly acknowledged writes.
Provision a
fresh database with `sync-server migrate` before the first release. Catalogue
and metadata services are deployed independently. The
script defaults to `192.168.1.68`;
override the SSH account when needed:

```sh
BOKHEIM_DEPLOY_USER=johan servers/sync/deploy/deploy-remote.sh
```

Provision the non-destructive sync activation helper once on the host:

```sh
sudo servers/sync/deploy/provision-server-deploy.sh johan
```

Reconnect SSH afterward. The provisioner grants passwordless sudo only for
`/usr/local/sbin/bokheim-deploy-sync`; staged shell scripts are never run as
root. Routine deployments then need only the SSH key. The script does not
modify Caddy, replace credentials, delete asset files, or reset the database.

### Debian-compatible release binaries

The production host is Debian 11-era and may provide an older glibc than the
development machine. Do not copy host-native release binaries when the build
machine reports a newer glibc requirement; a binary requiring `GLIBC_2.43`, for
example, will not start on the host.

PDFium dynamically and must use a compatible GNU build environment:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl -p sync-server
python3 shared/pdfium/prepare.py --target x86_64-unknown-linux-gnu \
  --dest target/x86_64-unknown-linux-gnu/release/pdfium --offline
cargo build --release --target x86_64-unknown-linux-musl -p metadata-server
```

The release helper currently builds host-native artifacts itself. When
deploying from a different libc environment, stage the musl artifacts through
the target host's passwordless activation workflow, or update the helper to
build and select the configured target. Never activate a binary until its
linkage has been checked against the target host.

This deployment exposes only Caddy to the Internet. The Rust server accepts
requests through `/run/bokheim/sync.sock`, whose filesystem permissions allow
access only to the `bokheim` user and members of the `bokheim` group. PostgreSQL
remains reachable only on localhost.

## Host preparation

Install PostgreSQL and Caddy from their official packages, then create the
unprivileged application account:

```sh
sudo useradd --system --home-dir /var/lib/bokheim --shell /usr/bin/nologin bokheim
sudo install -d -o bokheim -g bokheim -m 0750 /var/lib/bokheim
sudo install -d -o root -g root -m 0700 /etc/bokheim
```

### Large-disk asset storage

Keep the systemd home-directory sandbox enabled. Store the physical assets on
the large disk and bind-mount them into the service's state directory instead
of giving the service access to `/home`:

```sh
sudo chmod 0755 /home/johan/data
sudo install -d -o bokheim -g bokheim -m 0750 /home/johan/data/bokheim
sudo install -d -o bokheim -g bokheim -m 0750 /var/lib/bokheim/assets
```

Add this persistent mount to `/etc/fstab`:

```fstab
/home/johan/data/bokheim /var/lib/bokheim/assets none bind,nosuid,nodev,noexec 0 0
```

Activate and verify it before starting Bokheim:

```sh
sudo mount /var/lib/bokheim/assets
findmnt --mountpoint /var/lib/bokheim/assets
sudo install -d -o bokheim -g bokheim -m 0750 /var/lib/bokheim/assets/books /var/lib/bokheim/assets/thumbnails
```

The service unit requires this mount and refuses to start if it is absent. Its
absolute `BOKHEIM_ASSETS_DIR` value prevents the process working directory from
deciding where uploads are stored. Application
startup also probes both directories for create, write, sync, and delete access.

Build the release executable and install it without making it writable by the
service account:

```sh
cargo build --release --manifest-path servers/sync/Cargo.toml -p sync-server
sudo install -o root -g root -m 0755 servers/sync/target/release/sync-server /usr/local/bin/sync-server
```

Create `/etc/bokheim/database-url` containing a PostgreSQL URL that resolves to
localhost. Protect the file:

```sh
sudo chmod 0600 /etc/bokheim/database-url
sudo chown root:root /etc/bokheim/database-url
```

Create a separate persistent 32-byte key for account PIN, password-reset, and
mail-outbox protection. Do not replace this key while pending registrations,
password resets, or mail rows exist:

```sh
openssl rand -base64 32 | sudo tee /etc/bokheim/account-token-key >/dev/null
sudo chmod 0600 /etc/bokheim/account-token-key
sudo chown root:root /etc/bokheim/account-token-key
```

The service receives it through the read-only `account_token_key` systemd
credential; it is not stored in PostgreSQL.

After the non-destructive sync deployer has been provisioned, activate a
staged sync release with `sudo /usr/local/sbin/bokheim-deploy-sync`.

The built-in defaults use 32 maximum and 2 warm PostgreSQL connections,
16 staged books/4 GiB globally, 4 staged books/2 GiB per account, and at most
16 concurrent thumbnail uploads globally/4 per account, with additional
declared-byte ceilings of 512 MiB globally/256 MiB per account. Thumbnail
uploads have a two-minute total deadline. Override `BOKHEIM_DATABASE_*`,
`BOKHEIM_MAX_STAGED_*`, `BOKHEIM_MAX_THUMBNAIL_UPLOADS*`, or
`BOKHEIM_MAX_IN_FLIGHT_THUMBNAIL_BYTES*` in a systemd drop-in only after
measuring production saturation; a larger database pool consumes more
PostgreSQL memory.

## Public registration

Public registration is always enabled and requires email verification. The
sync server delivers verification and password-reset messages through Resend,
using its transactional PostgreSQL outbox for retries and idempotency.

Add and verify `mail.bokheim.se` in Resend, then publish the DNS records Resend
provides. The checked-in service sends as
`Bokheim <accounts@mail.bokheim.se>`; change `BOKHEIM_EMAIL_FROM` in the unit if
a different verified domain is chosen. Create a sending-only API key in Resend
and install it on the host without a trailing command or variable name:

```sh
sudo install -d -o root -g root -m 0700 /etc/bokheim
sudo install -o root -g root -m 0600 resend-api-key /etc/bokheim/resend-api-key
```

The systemd unit exposes that file to the service as a read-only credential.
The service is allowed outbound network access because Resend's HTTPS endpoint
does not have stable addresses suitable for `IPAddressAllow`; PostgreSQL and
the sync HTTP listener remain local-only.

## Configured test account

The checked-in service configures `test@bokheim.se` at startup with a 1000 GB
storage quota. Store its password in a second protected credential:

```sh
sudo install -o root -g root -m 0600 test-user-password /etc/bokheim/test-user-password
```

The credential file contains only the password and must not be empty. The
configured test user is exempt from the public-registration password minimum.
Change `BOKHEIM_TEST_USER_EMAIL` in the unit to use a different identifier.
Changing the credential rotates the password at the next
restart and revokes that account's existing sessions; an unchanged password
preserves sessions.

## Install service configuration

```sh
sudo install -o root -g root -m 0644 servers/sync/deploy/bokheim-sync.service /etc/systemd/system/bokheim-sync.service
sudo install -d -o root -g root -m 0755 /etc/systemd/system/caddy.service.d
sudo install -o root -g root -m 0644 servers/sync/deploy/caddy-bokheim.conf /etc/systemd/system/caddy.service.d/bokheim.conf
sudo install -o root -g root -m 0644 servers/sync/deploy/Caddyfile /etc/caddy/Caddyfile
sudo install -o root -g root -m 0600 servers/sync/deploy/caddy.env.example /etc/bokheim/caddy.env
```

Edit `/etc/bokheim/caddy.env` and replace both example values. Point the domain's
public A and/or AAAA records at this host. Permit inbound TCP ports 80 and 443,
but do not expose PostgreSQL or any Rust application port. Point DNS directly
at this host; adding a CDN or another proxy requires an explicit review of the
forwarded-address trust chain.

Validate and start the deployment:

```sh
sudo systemctl daemon-reload
sudo caddy validate --envfile /etc/bokheim/caddy.env --config /etc/caddy/Caddyfile
sudo systemctl enable --now bokheim-sync.service
sudo systemctl enable caddy.service
sudo systemctl restart caddy.service
curl --fail https://api.bokheim.se/api/health
curl --fail https://meta.bokheim.se/health
```

The expected health response is `ok`. Bokheim clients use the fixed production
origin `https://api.bokheim.se`; there is no end-user server URL setting.

`/api/live` reports only that the process is serving HTTP. `/api/ready` and the
backwards-compatible `/api/health` additionally require a working PostgreSQL
connection and writable, syncable book and thumbnail storage. Deployment and
monitoring must use readiness when deciding whether the server can accept work.
Readiness results are cached for five seconds so frequent or concurrent probes
cannot amplify into repeated durable writes on the asset volume.

Run the complete deployment gate after DNS and certificate issuance settle:

```sh
sudo servers/sync/deploy/verify-public.sh api.bokheim.se meta.bokheim.se
```

## Deploy the GPUI web application

Point `app.bokheim.se` at the same host as `api.bokheim.se`, then provision the
deployment account once on the host:

```sh
sudo servers/sync/deploy/provision-web-deploy.sh johan
```

Reconnect SSH so the new group membership applies. Future static application
deployments build the GPUI WebAssembly target, need no root access, and do not restart either Rust service,
PostgreSQL, or Caddy:

```sh
servers/sync/deploy/deploy-web-remote.sh
```

The installer atomically switches `/usr/local/share/bokheim-web/current` and
rolls back if the public application cannot be fetched. Caddy follows that
symlink and proxies same-origin `/api/*` requests to the sync Unix socket.

It verifies both services, the Caddy configuration, Unix-socket permissions,
HTTP-to-HTTPS redirection, the HTTPS health response, HSTS, certificate validity,
and that neither port 8080 nor PostgreSQL is publicly listening.

## Verification and maintenance

Inspect startup and certificate issuance without printing credentials:

```sh
systemctl status bokheim-sync.service caddy.service
journalctl -u bokheim-sync.service -u caddy.service --since today
```

If the large disk is unavailable, `bokheim-sync.service` deliberately remains
stopped. Check the mount before attempting to restart it:

```sh
findmnt --mountpoint /var/lib/bokheim/assets
systemctl status var-lib-bokheim-assets.mount bokheim-sync.service
```

The HTTP edge and Rust service bound request headers, stalled request bodies,
total request duration, and concurrent in-flight requests. systemd also caps
memory, tasks, and file descriptors. SIGTERM stops new connections and gives
active requests 30 seconds to drain before systemd enforces shutdown.

Upgrade by installing a newly built executable and restarting only the Rust
service. Reload Caddy configuration without downtime:

```sh
sudo systemctl restart bokheim-sync.service
sudo systemctl reload caddy.service
```

To validate, install, reload, and verify an updated Caddyfile in one step, copy
the deployment directory to the server and run:

```bash
sudo ./install-caddy-config.sh
```

The installer retains the previous `/etc/caddy/Caddyfile` as a timestamped
backup and restores it automatically if the reload or metadata health check
fails. It reloads only Caddy and does not restart the Bokheim services.

## Database recovery and synchronization history

The sync activation helper preserves PostgreSQL state. Normal restarts,
and deployments preserve the `sync_history`
generation UUID. Do not rotate it during an ordinary release.

After restoring a PostgreSQL backup or replacing database history:

1. Stop `bokheim-sync.service` and any other synchronization-server process
   that uses the same database. Keep traffic stopped throughout restoration.
2. Restore a backup with the current schema, then run `sync-server verify-schema`
   with its database credentials before continuing.
3. Explicitly rotate the history generation using the installed database
   credential, then restart synchronization:

```sh
sudo systemd-run --wait --pipe --collect --service-type=exec --uid=bokheim \
  --setenv=RUST_LOG=info \
  --property=LoadCredential=database_url:/etc/bokheim/database-url \
  /usr/local/bin/sync-server rotate-sync-generation
sudo systemctl start bokheim-sync.service
```

Only restart the service if the rotation command succeeds. It updates one
singleton UUID without changing state values, authored versions, or sequence
numbers. New databases receive a fresh UUID during `migrate`; a database
recreated from scratch therefore already has a new history.

Rotation is required after every backup restore: the backup contains the old
UUID, and its numeric head can later pass a client's checkpoint without
restoring the missing cells. Clients with the old generation receive HTTP 410
and reconcile from zero. They retain local state and queued edits, then
republish canonical cells with their original authored versions. Client-retained
state can repair missing server state; data absent from both the backup and
all clients cannot be recovered by this protocol.

### Database initialization

Run `sync-server migrate` only against an empty database. Existing databases
must already use the current schema. Deployment and startup verify that schema;
historical schema upgrades are not supported.
