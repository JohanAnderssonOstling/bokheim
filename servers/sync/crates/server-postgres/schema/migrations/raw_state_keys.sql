-- Book register keys are digests; directory and annotation keys are UTF-8.
DROP TRIGGER lww_guard ON sync_state;
ALTER TABLE sync_state ALTER COLUMN entity_key TYPE bytea USING
 CASE WHEN kind IN ('book_facts','book_lifecycle','placement','reading_position','metadata','description','pdf_reader_metadata','book_toc')
 THEN decode(entity_key,'hex') ELSE convert_to(entity_key,'UTF8') END;
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=7;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=7);
