-- Format 8 stores complete endpoints, including different subclasses and Cutters.
BEGIN IMMEDIATE;
CREATE TABLE lcc_range_v8(
 start_code TEXT NOT NULL CHECK(length(trim(start_code))>0),
 end_code TEXT NOT NULL CHECK(length(trim(end_code))>0),
 concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
 PRIMARY KEY(start_code,end_code)
) STRICT, WITHOUT ROWID;
INSERT INTO lcc_range_v8 SELECT class_letters || start_value,class_letters || end_value,concept_id FROM lcc_range;
DROP TABLE lcc_range;
ALTER TABLE lcc_range_v8 RENAME TO lcc_range;
CREATE INDEX lcc_range_concept_idx ON lcc_range(concept_id);
UPDATE taxonomy_meta SET value='8' WHERE key='format_version';
COMMIT;
