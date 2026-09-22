-- Consolidate jurisdiction-specific insolvency schedules using their official
-- LCC top-level spans. Isolated KBM/KBP 955.x selectors are retained.
BEGIN;

DELETE FROM source_selector
WHERE concept_id = 5613
  AND system_id = 'lcc'
  AND (
    selector GLOB 'KBM1*' OR
    selector GLOB 'KBP1*' OR
    selector GLOB 'KJC4*' OR
    selector GLOB 'KK4*'
  );

INSERT INTO source_selector(concept_id, system_id, selector) VALUES
  (5613, 'lcc', 'KBM1885..KBM1942'),
  (5613, 'lcc', 'KBP1885..KBP1942'),
  (5613, 'lcc', 'KJC4175..KJC4353'),
  (5613, 'lcc', 'KK4171..KK4389.2');

COMMIT;
