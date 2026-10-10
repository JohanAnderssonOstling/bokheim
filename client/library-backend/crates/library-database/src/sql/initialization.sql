-- Library initialization, local policy reset and database integrity checks.

-- name: initialization_reset_enrichment_policy &
UPDATE local_book_draft SET retry_at=NULL
WHERE NOT EXISTS(SELECT 1 FROM library_local_state WHERE singleton=1 AND enrichment_policy='books_v3');
UPDATE book_transfer SET cover_checked=0
WHERE NOT EXISTS(SELECT 1 FROM library_local_state WHERE singleton=1 AND enrichment_policy='books_v3');
UPDATE library_local_state SET enrichment_policy='books_v3'
WHERE singleton=1 AND enrichment_policy IS NOT 'books_v3';

-- name: initialization_replica_id ->
SELECT replica_id FROM sync_metadata WHERE singleton=1;

-- name: initialization_seed_library &
INSERT OR IGNORE INTO sync_clock VALUES(1,0);
            INSERT OR IGNORE INTO sync_metadata(singleton) VALUES(1);
            INSERT OR IGNORE INTO library_local_state(singleton,enrichment_policy) VALUES(1,'books_v3');
            INSERT INTO dir(id,parent_id,name,canonical_parent_id,canonical_name,lifecycle,deleted_at)
            VALUES('00000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','','00000000-0000-0000-0000-000000000000','',0,NULL)
            ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,name=excluded.name,deleted_at=NULL;

-- name: initialization_quick_check ->
PRAGMA quick_check;

-- name: initialization_foreign_key_violations ->
SELECT count(*) FROM pragma_foreign_key_check;

-- name: initialization_checkpoint &
PRAGMA wal_checkpoint(TRUNCATE);
