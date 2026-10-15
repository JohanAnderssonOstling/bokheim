-- Reviewed broad LC classes use existing taxonomy roots without inferring a
-- language, genre or audience. Numbered selectors and narrower ranges remain.
-- D: https://loc.gov/aba/cataloging/classification/lcco/lcco_d.pdf
-- CT: https://www.loc.gov/aba/cataloging/classification/lcco/lcco_c.pdf
-- PJ, PL, PZ: https://www.loc.gov/aba/cataloging/classification/lcco/lcco_p.pdf
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'D' FROM concept
WHERE concept_id = 3319 AND preferred_label = 'History';

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'CT' FROM concept
WHERE concept_id = 3212 AND preferred_label = 'Biography';

DELETE FROM lcc_selector WHERE concept_id = 5583 AND selector = 'PZ';
INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'PZ' FROM concept
WHERE concept_id = 3846 AND preferred_label = 'Literature';

-- The numeric umbrella currently has the same excluded Genres destination.
-- Move that broad range too; retain PZ3 and all more specific refinements.
UPDATE lcc_range SET concept_id = 3846
WHERE concept_id = 5583
  AND start_letters = 'PZ' AND start_number = 1 AND start_cutters = '[]'
  AND end_letters = 'PZ' AND end_number = 90 AND end_cutters = '[]';

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'PJ' FROM concept
WHERE concept_id = 3702 AND preferred_label = 'Language';

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
SELECT concept_id, 'PL' FROM concept
WHERE concept_id = 3702 AND preferred_label = 'Language';

COMMIT;
