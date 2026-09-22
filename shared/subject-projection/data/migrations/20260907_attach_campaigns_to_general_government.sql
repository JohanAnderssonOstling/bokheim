BEGIN;

-- Civic Participation already owns the exact official JF799-JF1197.2 and
-- JF1001-JF1048.52 ranges; expose that existing leaf here instead of creating
-- a competing elections destination.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 5329,5691,COALESCE(MAX(ordinal),-1)+1
FROM concept_parent
WHERE concept_id=5329
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent
    WHERE concept_id=5329 AND parent_concept_id=5691
  );

COMMIT;
