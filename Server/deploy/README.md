# Public Internet deployment

## Remote code deployment

After the host has been prepared once with `install-server.sh`, deploy a new
release from this checkout with:

```sh
Server/deploy/deploy-remote.sh
```

The script builds `sync-server` and `catalog-server`, transfers the
release artifacts over SSH, and runs `install-staged.sh` with `sudo` on the
host. It defaults to `192.168.1.68`; override the SSH account when needed:

```sh
BOKHEIM_DEPLOY_USER=johan Server/deploy/deploy-remote.sh
```

The remote account must have SSH access and passwordless `sudo` (or an
interactive sudo configuration; the installer allocates a TTY for a remote
sudo password prompt. Do not prefix the local command with `sudo`. The script does not provision a new host or
modify database, Caddy, or credentials.

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
cargo build --release --manifest-path Server/Cargo.toml -p sync-server
sudo install -o root -g root -m 0755 Server/target/release/sync-server /usr/local/bin/sync-server
```

Create `/etc/bokheim/database-url` containing a PostgreSQL URL that resolves to
localhost. Protect the file:

```sh
sudo chmod 0600 /etc/bokheim/database-url
sudo chown root:root /etc/bokheim/database-url
```

The service loads only this database credential. It does not create a built-in
administrator account or read an administrator password.

The built-in defaults use 32 maximum and 2 warm PostgreSQL connections,
16 staged books/4 GiB globally, and 4 staged books/2 GiB per account. Override
`BOKHEIM_DATABASE_*` or `BOKHEIM_MAX_STAGED_*` in a systemd drop-in only after
measuring production saturation; a larger database pool consumes more
PostgreSQL memory.

## Public registration

Public registration is always enabled. Configure an external email worker for
the `account_email_outbox` table. The
worker owns provider credentials, SMTP authentication, TLS, delivery retries,
and bounce handling; the Bokheim server only creates and manages queued
verification/reset messages.

Before running `install-server.sh`, create
`/etc/bokheim/disposable-email-domains` as a non-empty list containing
one lowercase domain per line. Blank lines and lines beginning with `#` are
ignored. Treat this list as abuse reduction, not proof that a provider is
permanent; mailbox verification remains authoritative.

```sh
sudo install -d -o root -g root -m 0700 /etc/bokheim
sudo install -o root -g root -m 0644 disposable-email-domains /etc/bokheim/disposable-email-domains
```

The checked-in service unit supplies this fixed path through
`BOKHEIM_DISPOSABLE_EMAIL_DOMAINS_FILE`.

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
sudo systemctl enable --now catalog-providers.service
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

## No backups: the database is disposable

This is a development deployment target, not a production one. `install-server.sh`
drops and recreates the `bokheim` database on every deploy, so there is no
schema migration history and nothing to back up -- redeploying is the recovery
procedure. Do not point this at a deployment expected to hold data that must
survive a redeploy without a design change to persist it first.
