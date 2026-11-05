# Cloud library data upgrade

This standalone maintenance executable upgrades PostgreSQL schema 14 to 17.
It uses the current client replica to validate and re-encode stored protocol
28–31 payloads. The synchronization relay continues to treat payloads as opaque.

`library-cloud-upgrade check` uses a read-only transaction. `apply` locks the
affected tables and commits the payload conversion and schema version together.
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
