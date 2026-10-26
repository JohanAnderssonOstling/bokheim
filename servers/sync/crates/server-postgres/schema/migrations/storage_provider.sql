ALTER TABLE libraries ADD COLUMN storage_configuration jsonb NOT NULL DEFAULT '"bokheim"';
UPDATE libraries SET storage_configuration='"local"' WHERE NOT cloud_storage_enabled;
ALTER TABLE libraries ADD CONSTRAINT external_storage_excludes_bokheim CHECK (NOT (storage_configuration ? 'external') OR NOT cloud_storage_enabled);

ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=12;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=12);
