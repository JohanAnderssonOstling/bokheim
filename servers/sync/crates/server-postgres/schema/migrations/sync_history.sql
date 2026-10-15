CREATE TABLE sync_history (
    singleton boolean PRIMARY KEY CHECK (singleton),
    generation uuid NOT NULL
);
INSERT INTO sync_history(singleton, generation) VALUES (true, gen_random_uuid());

ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=11;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=11);
