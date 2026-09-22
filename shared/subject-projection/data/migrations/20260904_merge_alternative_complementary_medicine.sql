-- Merge the duplicate BISAC umbrella into the curated parent concept.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES ('bokheim:subject:health:complementary', 'bisac', 'MED004000', 0);

DELETE FROM concept_parent
WHERE concept_id='unified:18f3bcb3-c2c0-5fda-8483-80613369530a';
DELETE FROM concept
WHERE concept_id='unified:18f3bcb3-c2c0-5fda-8483-80613369530a';

COMMIT;
