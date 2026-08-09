# Independent catalog-service deployment

The catalog server is an independent public process. It has no database or
authentication secrets and does not run as the synchronization-server user.
Caddy supplies public HTTPS while the Rust process listens only on loopback.
Normalized catalogue pages and covers are retained under
`/var/lib/bokheim-catalogs`; systemd creates and owns that state directory for
the dynamic service user.

Build and install it separately from the synchronization server:

```sh
cargo build --release -p bokheim-catalog-server
sudo install -o root -g root -m 0755 target/release/bokheim-catalog-server /usr/local/bin/bokheim-catalog-server
sudo install -d -o root -g root -m 0755 /usr/local/share/bokheim-catalogs
sudo install -o root -g root -m 0644 Catalogs/opds.json /usr/local/share/bokheim-catalogs/opds.json
sudo install -o root -g root -m 0644 Catalogs/deploy/bokheim-catalogs.service /etc/systemd/system/bokheim-catalogs.service
```

Install [Caddyfile](Caddyfile) and [caddy.env.example](caddy.env.example) on the
catalog host, or merge the site block into an existing Caddy configuration.
Point `catalogs.bokheim.se` at that host, then enable both services.

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now bokheim-catalogs.service
sudo systemctl reload caddy.service
curl --fail https://catalogs.bokheim.se/health
curl --fail --compressed --header 'Accept: application/vnd.bokheim+bincode; version=5' --header 'Accept-Encoding: zstd' --output /dev/null https://catalogs.bokheim.se/api/opds/catalogs
```

To publish a new list, prepare and validate a separate file before replacing
the live registry atomically:

```sh
bokheim-catalog-server --check-catalog ./opds.next.json
sudo install -o root -g root -m 0644 ./opds.next.json /usr/local/share/bokheim-catalogs/opds.next.json
sudo mv /usr/local/share/bokheim-catalogs/opds.next.json /usr/local/share/bokheim-catalogs/opds.json
```

No process restart is needed. The next request activates the valid revision;
invalid file contents leave the previous valid registry in service.
