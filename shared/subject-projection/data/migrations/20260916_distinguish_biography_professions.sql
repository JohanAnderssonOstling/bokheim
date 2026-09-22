-- Keep the distinct exploration and interior-decoration subjects beneath Biography.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label = 'Explorers'
WHERE concept_id = 6988 AND preferred_label IN ('Collective', 'Explorers');
UPDATE concept SET preferred_label = 'Interior Designers'
WHERE concept_id = 7503 AND preferred_label IN ('Collective', 'Interior Designers');
COMMIT;
