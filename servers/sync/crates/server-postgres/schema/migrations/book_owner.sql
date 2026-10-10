-- Schema 1 -> 2: every synchronized value names its book in content_hash, and
-- a lifecycle's quota presence follows from its purge rank instead of a
-- separate `present` column. Executed transactionally after the baseline.
DROP TRIGGER lww_guard ON sync_state;
DROP TRIGGER lww_guard ON sync_reading_state;

-- Removals, purges and schema-1 rows without a declaration named no book.
UPDATE sync_state SET content_hash = entity_key
WHERE content_hash IS NULL
  AND kind IN ('book_lifecycle', 'book_facts', 'placement', 'metadata', 'description', 'pdf_reader_metadata', 'book_toc');
UPDATE sync_reading_state SET content_hash = entity_key WHERE content_hash IS NULL AND kind = 'reading_position';

ALTER TABLE sync_state DROP COLUMN present;
ALTER TABLE sync_reading_state DROP COLUMN present;
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank, event_id ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank, event_id ON sync_reading_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();

ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version = 2;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK (version = 2);
