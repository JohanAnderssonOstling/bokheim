# Public Internet deployment

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
absolute `BOKHEIM_BOOKS_DIR` and `BOKHEIM_THUMBNAILS_DIR` values prevent the
process working directory from deciding where uploads are stored. Application
startup also probes both directories for create, write, sync, and delete access.

Build the release executable and install it without making it writable by the
service account:

```sh
cargo build --release --manifest-path Server/Cargo.toml -p sync-server
sudo install -o root -g root -m 0755 Server/target/release/sync-server /usr/local/bin/sync-server
```

Create `/etc/bokheim/database-url` containing a PostgreSQL URL that resolves to
localhost. For the first startup only, create `/etc/bokheim/admin-password`
containing a unique initial admin password of at least 12 characters and add a
temporary systemd drop-in which loads it as the `admin_password` credential.
Protect both files:

```sh
sudo chmod 0600 /etc/bokheim/database-url /etc/bokheim/admin-password
sudo chown root:root /etc/bokheim/database-url /etc/bokheim/admin-password
```

The admin credential is used only if `bokheim-admin` does not already exist.
After the first successful startup, delete the temporary drop-in and password
file, then run `systemctl daemon-reload`. The production unit deliberately loads
only the database credential. The provided `install-server.sh` performs this
bootstrap cleanup automatically.

The checked-in unit makes the database pool and upload concurrency explicit:
32 maximum and 2 warm PostgreSQL connections, at most 2 simultaneous book
validations, 16 staged books/4 GiB globally, and 4 staged books/2 GiB per
account. Tune `BOKHEIM_DATABASE_*`, `BOKHEIM_MAX_CONCURRENT_BOOK_VALIDATIONS`,
and `BOKHEIM_MAX_STAGED_*` in a systemd drop-in only from
measured production saturation; validation is CPU-heavy and a larger database
pool also consumes PostgreSQL memory.

## Optional public registration

Keep public registration disabled for an invite-only deployment. To enable it,
install a local queueing MTA that provides `/usr/sbin/sendmail`. The MTA, rather
than Bokheim, owns remote SMTP authentication, TLS, delivery retries, and bounce
handling. Bokheim only invokes the local queue, which is compatible with the
service's outbound-network sandbox.

Maintain `/etc/bokheim/disposable-email-domains` as a non-empty list containing
one lowercase domain per line. Blank lines and lines beginning with `#` are
ignored. Treat this list as abuse reduction, not proof that a provider is
permanent; mailbox verification remains authoritative.

Add a systemd drop-in:

```ini
[Service]
Environment=BOKHEIM_PUBLIC_REGISTRATION=true
Environment=BOKHEIM_MAIL_FROM=accounts@bokheim.se
Environment=BOKHEIM_DISPOSABLE_EMAIL_DOMAINS_FILE=/etc/bokheim/disposable-email-domains
```

If the local MTA exposes a different sendmail-compatible executable, also set
`BOKHEIM_SENDMAIL_PATH` to its absolute path. The server fails startup when any
required registration dependency is missing or malformed.

## Install service configuration

```sh
sudo install -o root -g root -m 0644 Server/deploy/bokheim-sync.service /etc/systemd/system/bokheim-sync.service
sudo install -d -o root -g root -m 0755 /etc/systemd/system/caddy.service.d
sudo install -o root -g root -m 0644 Server/deploy/caddy-bokheim.conf /etc/systemd/system/caddy.service.d/bokheim.conf
sudo install -o root -g root -m 0644 Server/deploy/Caddyfile /etc/caddy/Caddyfile
sudo install -o root -g root -m 0600 Server/deploy/caddy.env.example /etc/bokheim/caddy.env
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
sudo systemctl enable --now bokheim-catalogs.service
sudo systemctl enable caddy.service
sudo systemctl restart caddy.service
curl --fail https://api.bokheim.se/api/health
curl --fail https://catalogs.bokheim.se/health
curl --fail --compressed --header 'Accept: application/vnd.bokheim+bincode; version=5' --header 'Accept-Encoding: zstd' --output /dev/null https://catalogs.bokheim.se/api/opds/catalogs
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
sudo Server/deploy/verify-public.sh api.bokheim.se catalogs.bokheim.se
```

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

Old unreferenced assets and abandoned upload files are collected daily by
`bokheim-gc.timer`, with a 24-hour safety window. The database advisory lease
prevents synchronization changes during the live-reference snapshot. A shared
filesystem lock prevents GC from deleting files while backup or restore tooling
is reading them:

```sh
systemctl list-timers bokheim-gc.timer
journalctl -u bokheim-gc.service --since '7 days ago'
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

## Backup and disaster recovery

The deployment includes an optional off-host backup system built from
PostgreSQL `pg_dump` and restic. It takes coordinated database-and-asset
snapshots, applies retention, reads repository data on a rotating schedule, and
proves monthly that the database can actually be restored. It is deliberately
not enabled until a separate repository destination is configured.

See [BACKUP.md](BACKUP.md) for SFTP/NAS setup, timer operation, restore drills,
and the disaster-recovery procedure.
