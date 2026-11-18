# Cloud library data upgrade

This standalone maintenance executable upgrades PostgreSQL schema 14 to 17 and
converts legacy sync payloads on schemas 20–22 to typed storage version 34. It reads
generic payload versions 28–33 only inside this offline tool, validates their
current domain values, and re-encodes them as typed protobuf. Normal clients
accept only storage version 34; they have no legacy decoder fallback.

The storage version is independent of HTTP. The relay continues to treat payloads
as opaque, and the local bincode database layout is unchanged.

`library-cloud-upgrade check` uses a read-only transaction. `apply` locks the
affected tables and commits conversions transactionally. Schema-20–22 conversion
keeps the database schema and authored state versions unchanged, archives each
original row in `bokheim_upgrade_archive.sync_state_before_typed_payloads`, and
changes the sync-history generation so clients pull the converted states again.
Both commands read `DATABASE_URL` from the environment and reject unreadable
records or mismatched identities and conflict ranks.

Use the sync deployment helper for activation. It stops the server, takes a full
database backup, runs the upgrade, and checks the private candidate before
allowing public requests. Do not invoke `apply` against a serving instance.

Accounts, library IDs, authored versions, quotas and asset references are
retained. The original synchronization rows and storage configurations remain
in `bokheim_upgrade_archive`. Removed external-storage configuration is archived
and its library uses the current local storage policy; external files are not
touched. Existing `opds_feeds` rows are retained. The synchronization generation
changes so replicas pull the converted payloads again.

Reapplying to a valid schema-17 database makes no changes. Unknown schemas,
unsupported policies, or failed conversions leave the transaction uncommitted.

Before activating clients using storage 34, run `check`, stop the serving instance,
take a backup, then run `apply`. An unsupported or unconvertible record aborts the
operation without applying any updates. Reapplying to an already converted
schema-20 database makes no changes. This repository change does not run either
command against a database.

Payload readiness is recorded separately in `sync_payload_migration`. On schemas
20–22, `apply` writes the current storage version only after all rows have been
validated and conversions have succeeded, within the same locked transaction.
Already-typed databases without the marker are verified and marked without
rotating history; conversions still rotate history. `check` never writes it.
Empty databases created by `sync-server migrate` start with the current marker.

`sync-server upgrade` checks this marker independently of the relational schema,
and `verify-schema`, startup and readiness reject incomplete payload migration.
Deployment runs the upgrade after stopping service and taking its backup, before
starting the candidate. Rollback removes a newly introduced marker table before
restoring a backup that did not contain it. HTTP versioning is unaffected.
