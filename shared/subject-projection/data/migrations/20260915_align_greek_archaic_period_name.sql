-- Align the Greek period name with the LCC heading for DF222–DF224.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
VALUES (10344, 'Archaic Era, c. 800–500 BC', 0);

UPDATE concept
SET preferred_label = 'Age of Tyrants'
WHERE concept_id = 10344
  AND preferred_label = 'Archaic Era, c. 800–500 BC';

COMMIT;
