-- Normalize the remaining 1810/25 literature period labels.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='1810–1960'
WHERE concept_id IN (11951,11956,11965,11978);

COMMIT;
