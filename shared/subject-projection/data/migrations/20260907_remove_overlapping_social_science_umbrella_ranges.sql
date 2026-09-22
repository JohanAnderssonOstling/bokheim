BEGIN;
-- These are navigational ancestors.  Keep their printed broad notations for
-- exact matching, but do not let them permissively claim codes owned by leaves.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code || '-' || end_code
FROM lcc_range
WHERE concept_id IN (5255,5463,5488);
DELETE FROM lcc_range WHERE concept_id IN (5255,5463,5488);
COMMIT;
