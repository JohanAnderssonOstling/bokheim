-- Use the LCC-aligned title-case name for the JA subject.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
UPDATE concept
SET preferred_label = 'Political Science'
WHERE concept_id = 5369 AND preferred_label = 'Political science';
COMMIT;
