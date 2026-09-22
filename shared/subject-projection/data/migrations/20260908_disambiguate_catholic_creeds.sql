PRAGMA foreign_keys=ON;
BEGIN;
UPDATE concept
SET preferred_label='Catholic Creeds & Catechisms'
WHERE concept_id=12909
  AND preferred_label='Creeds & Catechisms'
  AND EXISTS (
    SELECT 1 FROM concept_parent
    WHERE concept_id=12909 AND parent_concept_id=54050
  );
COMMIT;
