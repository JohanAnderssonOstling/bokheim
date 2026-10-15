-- Explicit deployment upgrade; raw BLAKE3 digest storage.
DROP TRIGGER lww_guard ON sync_state;
ALTER TABLE user_blob_charge DROP CONSTRAINT user_blob_charge_content_hash_fkey;
ALTER TABLE library_blob_claim DROP CONSTRAINT library_blob_claim_content_hash_fkey;
ALTER TABLE blob_object ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE user_blob_charge ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE user_blob_reference ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE book_upload_reservation ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE library_blob_claim ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE sync_state ALTER COLUMN content_hash TYPE bytea USING decode(content_hash, 'hex');
ALTER TABLE user_blob_charge ADD FOREIGN KEY (content_hash) REFERENCES blob_object(content_hash) ON DELETE RESTRICT;
ALTER TABLE library_blob_claim ADD FOREIGN KEY (content_hash) REFERENCES blob_object(content_hash);
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=6;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=6);
