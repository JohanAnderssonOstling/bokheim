-- Materialize the curated graph after the complete taxonomy has been copied to
-- master-taxonomy-v2.sqlite3. Browse exclusions are not part of the final data
-- model: excluded concepts are absent from the curated database.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM browse_exclusion)
   OR parent_concept_id IN (SELECT concept_id FROM browse_exclusion);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM browse_exclusion);

DROP TABLE browse_exclusion;

COMMIT;
