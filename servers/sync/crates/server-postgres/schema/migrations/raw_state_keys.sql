-- Book register keys are digests; directory and annotation keys are UTF-8.
ALTER TABLE sync_state ALTER COLUMN entity_key TYPE bytea USING
 CASE WHEN kind IN ('book_facts','book_lifecycle','placement','reading_position','metadata','description','pdf_reader_metadata','book_toc')
 THEN decode(entity_key,'hex') ELSE convert_to(entity_key,'UTF8') END;
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=7;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=7);
