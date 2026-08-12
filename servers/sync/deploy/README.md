# Public Internet deployment

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
credentials before stopping services or replacing the database.

The script builds `sync-server` and `catalog-server`, transfers the
release artifacts over SSH, and runs `install-staged.sh` with `sudo` on the
host. The staged installer deliberately drops and recreates only the local
`bokheim` PostgreSQL database before migrating and restarting; book and
thumbnail files remain on disk. It refuses the destructive step unless the
database credential matches the expected local database exactly. The script
defaults to `192.168.1.68`; override the SSH account when needed:

```sh
BOKHEIM_DEPLOY_USER=johan servers/sync/deploy/deploy-remote.sh
```

The remote account must have SSH access and passwordless `sudo` (or an
interactive sudo configuration; the installer allocates a TTY for a remote
sudo password prompt. Do not prefix the local command with `sudo`. The script
does not provision a new host, modify Caddy, replace credentials, or delete
asset files; it does replace the `bokheim` database as described above.

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

The service loads only this database credential. It does not create a built-in
administrator account or read an administrator password.

The built-in defaults use 32 maximum and 2 warm PostgreSQL connections,
16 staged books/4 GiB globally, and 4 staged books/2 GiB per account. Override
`BOKHEIM_DATABASE_*` or `BOKHEIM_MAX_STAGED_*` in a systemd drop-in only after
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
book quota. Store its password in a second protected credential:

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
sudo systemctl enable --now bokheim-catalogs.service
sudo systemctl enable caddy.service
sudo systemctl restart caddy.service
curl --fail https://api.bokheim.se/api/health
curl --fail https://catalogs.bokheim.se/health
curl --fail --header 'Accept: application/vnd.bokheim+bincode; version=11' --output /dev/null https://catalogs.bokheim.se/api/opds/catalogs
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
sudo servers/sync/deploy/verify-public.sh api.bokheim.se catalogs.bokheim.se
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
