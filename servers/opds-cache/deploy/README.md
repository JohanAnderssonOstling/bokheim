# Independent OPDS cache deployment

The OPDS cache is an independent public process with its own SQLite database and
cover directory. It has no authentication secrets and does not run as the
synchronization-server user. Caddy supplies HTTPS while the Rust process listens
on loopback.

Author enrichment additionally requires a healthy loopback-only
`bokheim-metadata.service` and an integrity-checked snapshot at
`/home/johan/data/openlibrary/current.sqlite`; see
`../../metadata/README.md`. The catalogue remains usable if that optional
lookup service is temporarily unavailable.

```sh
cargo build --release -p catalog-server
sudo install -o root -g root -m 0755 target/release/catalog-server /usr/local/bin/catalog-server
sudo install -d -o root -g root -m 0755 /usr/local/share/catalog-providers
sudo install -o root -g root -m 0644 servers/opds-cache/opds.json /usr/local/share/catalog-providers/opds.json
sudo install -o root -g root -m 0644 servers/opds-cache/deploy/bokheim-catalogs.service /etc/systemd/system/bokheim-catalogs.service
```

Install [Caddyfile](Caddyfile) and [caddy.env.example](caddy.env.example), point
`catalogs.bokheim.se` at the host, then enable both services:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now bokheim-catalogs.service
sudo systemctl reload caddy.service
curl --fail https://catalogs.bokheim.se/health
curl --fail https://meta.bokheim.se/health
curl --fail --header 'Accept: application/vnd.bokheim+protobuf; version=14' \
  --output /dev/null https://catalogs.bokheim.se/api/opds/catalogs
```

Prepare and validate a replacement registry before installing it atomically:

```sh
catalog-server --check-catalog ./opds.next.json
sudo install -o root -g root -m 0644 ./opds.next.json /usr/local/share/catalog-providers/opds.next.json
sudo mv /usr/local/share/catalog-providers/opds.next.json /usr/local/share/catalog-providers/opds.json
```

No restart is needed. The next registry request activates a valid higher
revision; invalid contents retain the previous registry.

For a catalogue-only binary deployment, stage `catalog-server`, `opds.json`,
`bokheim-catalogs.service`, and `install-catalog-staged.sh` in one remote
directory, then run:

```sh
sudo ./install-catalog-staged.sh "$PWD"
```

This installer does not stop the sync server or modify PostgreSQL. It validates
the registry before installation and restores the previous catalogue release if
the local health check does not pass.
