# Bokheim taxonomy

Taxonomy is distributed as signed SQLite snapshots through the update manifest.
Caddy serves immutable artifacts from `/srv/bokheim-updates/public/taxonomy`
under `https://taxonomy.bokheim.se/releases/*`. Other paths return 404.

The unused taxonomy HTTP server and its resolve, lookup, and snapshot APIs have
been removed. Snapshot publication uses the update release tooling described in
[the update server README](../updates/README.md).

The [curation tools](curation/Cargo.toml), reviewed taxonomy database, and
[viewer](curation/VIEWER.md) remain available for preparing and reviewing snapshots.
