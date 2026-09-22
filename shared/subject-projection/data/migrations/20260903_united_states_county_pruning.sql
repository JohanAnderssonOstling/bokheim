-- Hide county subjects while retaining them in the master taxonomy for
-- classification. This migration is idempotent.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS browse_exclusion (
    concept_id TEXT PRIMARY KEY REFERENCES concept(concept_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
WITH RECURSIVE united_states_region(concept_id) AS (
    VALUES ('unified:62708214-9d20-5e1f-b7ef-d3e4d591bab9')
    UNION
    SELECT child.concept_id FROM concept_parent child JOIN united_states_region parent ON parent.concept_id = child.parent_concept_id
), regional_section(concept_id) AS (
    SELECT region.concept_id FROM united_states_region region JOIN concept ON concept.concept_id = region.concept_id
    WHERE concept.preferred_label = 'Regions & Administrative Divisions'
)
INSERT OR IGNORE INTO browse_exclusion (concept_id)
SELECT child.concept_id FROM regional_section section
JOIN concept_parent child ON child.parent_concept_id = section.concept_id
JOIN concept ON concept.concept_id = child.concept_id
WHERE lower(concept.preferred_label) LIKE '%county%' OR lower(concept.preferred_label) LIKE '%counties%';
COMMIT;
