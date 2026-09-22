-- Organize the 21st-century political and diplomatic subjects under the
-- thematic Political & Diplomatic History branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id = 3620
WHERE concept_id IN (10375, 10687)
  AND parent_concept_id = 10669;

COMMIT;
