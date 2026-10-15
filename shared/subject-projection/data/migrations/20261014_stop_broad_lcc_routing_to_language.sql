-- Broad Oriental and Asian language/literature classes should route to
-- Literature, not to the generic Language subject. Linguistics already owns
-- the specific P1-P1091 interval; remove Language's umbrella P-PZ interval.
BEGIN IMMEDIATE;

DELETE FROM lcc_selector
WHERE concept_id = 3702 AND selector IN ('PJ', 'PL');

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'PJ' FROM concept
WHERE concept_id = 3846 AND preferred_label = 'Literature';

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'PL' FROM concept
WHERE concept_id = 3846 AND preferred_label = 'Literature';

DELETE FROM lcc_range
WHERE concept_id = 3702
  AND start_letters = 'P' AND start_number IS NULL AND start_cutters = '[]'
  AND end_letters = 'PZ' AND end_number IS NULL AND end_cutters = '[]';

COMMIT;
