ALTER TABLE libraries ADD COLUMN kind jsonb NOT NULL DEFAULT '{"type":"standard"}';
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=15;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=15);
