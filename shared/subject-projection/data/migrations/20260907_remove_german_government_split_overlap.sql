BEGIN;
-- The schedule's history heading overlaps its chronological subheading.
-- Partition their permissive ranges so every code has exactly one owner.
UPDATE lcc_range
SET end_code='JN3356.9'
WHERE start_code='JN3241' AND end_code='JN3415.222'
  AND concept_id=(SELECT concept_id FROM concept WHERE preferred_label='History of German Government');
COMMIT;
