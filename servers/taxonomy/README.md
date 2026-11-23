# Bokheim taxonomy

Taxonomy is distributed as signed SQLite snapshots through the update manifest.
Caddy serves immutable artifacts from `/srv/bokheim-updates/public/taxonomy`
under `https://taxonomy.bokheim.se/releases/*`. Other paths return 404.

The unused taxonomy HTTP server and its resolve, lookup, and snapshot APIs have
been removed. Snapshot publication uses the update release tooling described in
[the update server README](../updates/README.md).

The [curation tools](curation/Cargo.toml), reviewed taxonomy database, and
[viewer](curation/VIEWER.md) remain available for preparing and reviewing snapshots.

## Runtime publication

The editable database remains authoring format 1. All clients consume the same
immutable runtime format 2, compiled on the publication host:

```sh
cargo run --release -p subject-projection --features compiler --bin compile-taxonomy -- \
  SOURCE.sqlite3 OUTPUT.sqlite3 RELEASE PREVIOUS_RUNTIME.sqlite3
```

For the first release (ID 1), omit `PREVIOUS_RUNTIME.sqlite3`. Subsequent releases
require the previous snapshot. Compilation rejects retired concept IDs without
redirects, including IDs already retired in previous releases. Concept IDs must
never be reused for different meanings. The previous input must also use runtime
format 2; authoring databases are not accepted as previous runtime snapshots.

The compiler builds navigation, selectors, range intervals and ancestry, embeds
reviewed aliases and heading rules, and removes authoring tables and indexes.
These reviewed rules contribute to the runtime content revision, so changing
them refreshes automatic assignments even when concept definitions are unchanged.
Projection policy revisions remain independent of the runtime storage format.

Publish the final output bytes, release ID, format 2, and minimum compatible
application/schema versions through the existing signed manifest. Do not edit
the output after hashing. Release builds can set `BOKHEIM_BUILD_RUNTIME_TAXONOMY=/path/to/OUTPUT.sqlite3`
to package the exact published bytes on every platform. Development builds without
that input compile the authoring snapshot through the same runtime contract;
the browser copies the prepared artifact unchanged rather than recompiling it.
The shared client selection record stores download provenance outside the image
and retains the previous reference for recovery. Clients accept only runtime
format 2 with matcher storage version 151. Legacy caches and selection records
are not migrated; startup recovers to a valid current-format snapshot or the
bundled runtime. The manifest continues to contain one taxonomy offer.

Native and browser storage adapt the same coordinator, validation and selection transitions.
Compatible taxonomy-only offers stage in the background; application packages retain
their explicit approval and host installation flow.
Kobo uses the shared native data-only host; its firmware launcher still installs
application packages. Taxonomy selection takes effect at the next app start or
browser reload, and every inspection worker uses that selected release.

Clients accept only catalog format 13, which records its minimum taxonomy release inside the snapshot,
without changing the manifest protocol. Catalogs requiring a newer selected
taxonomy remain pending. When the taxonomy activates, the shared catalog loader
installs the compatible pending source and browse projection in one SQLite
transaction. Previous catalog and taxonomy images are retained for recovery;
an invalid or incompatible pending image leaves the current catalog usable.
Rebuild catalog assignments from server curation when external-code mappings
change. Label and parent changes use existing concept identities directly.

Publish the catalog against the exact compiled runtime, so its dependency uses
that artifact's final release ID rather than the build bundle's release:

```sh
catalog CATALOG_DIR publish OUTPUT_DIR CATALOG_RELEASE --taxonomy OUTPUT.sqlite3
```

Metadata services can select the same published image with
`BOKHEIM_RUNTIME_TAXONOMY=/path/to/runtime.sqlite3` before startup. Restart the
service after changing the selected file; open references remain immutable.
macOS also uses the shared native data-only host for taxonomy updates.
