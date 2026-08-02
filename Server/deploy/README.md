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

Build the release executable and install it without making it writable by the
service account:

```sh
cargo build --release --manifest-path Server/Cargo.toml -p sync-server
sudo install -o root -g root -m 0755 Server/target/release/sync-server /usr/local/bin/sync-server
```

Create `/etc/bokheim/database-url` containing a PostgreSQL URL that resolves to
localhost. Create `/etc/bokheim/admin-password` containing a unique initial admin
password of at least 10 characters. Protect both files:

```sh
sudo chmod 0600 /etc/bokheim/database-url /etc/bokheim/admin-password
sudo chown root:root /etc/bokheim/database-url /etc/bokheim/admin-password
```

The admin credential is used only if `bokheim-admin` does not already exist. To
delete it after the first successful startup, first remove its `LoadCredential`
line from the installed unit, then run `systemctl daemon-reload`.

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
sudo systemctl enable caddy.service
sudo systemctl restart caddy.service
curl --fail https://sync.example.com/api/health
```

The expected health response is `ok`. Configure clients with
`SYNC_SERVER_URL=https://sync.example.com`.

Run the complete deployment gate after DNS and certificate issuance settle:

```sh
sudo Server/deploy/verify-public.sh sync.example.com
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

Upgrade by installing a newly built executable and restarting only the Rust
service. Reload Caddy configuration without downtime:

```sh
sudo systemctl restart bokheim-sync.service
sudo systemctl reload caddy.service
```
