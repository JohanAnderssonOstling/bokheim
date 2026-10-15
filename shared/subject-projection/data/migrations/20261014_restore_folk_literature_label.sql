-- Restore the reviewed label from literature-cleanup-20261014.json. Shortening
-- it to Folk collides with concept 4375 under General Studies (4369).
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Folk Literature', normalized_label = 'folk literature'
WHERE concept_id = 5065 AND preferred_label = 'Folk'
  AND EXISTS (
      SELECT 1 FROM concept_parent
      WHERE concept_id = 5065 AND parent_concept_id = 4369
  );

COMMIT;
