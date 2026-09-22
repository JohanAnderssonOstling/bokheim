-- All Ancient Italy LCC coverage is represented by its curated ranged children.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE concept_id=3571 AND system_id='lcc';
COMMIT;
