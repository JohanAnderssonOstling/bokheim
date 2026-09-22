BEGIN;

-- These captions are explicitly travel in the LCC schedule and must not be
-- inherited by the surrounding country-history ranges.
INSERT INTO concept(preferred_label)
SELECT 'Belarusian Travel'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Belarusian Travel');

INSERT INTO concept(preferred_label)
SELECT 'Maltese Travel'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Maltese Travel');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Belarusian Travel';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Maltese Travel';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK507.22' FROM concept WHERE preferred_label='Belarusian Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK507.29' FROM concept WHERE preferred_label='Belarusian Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK507.924' FROM concept WHERE preferred_label='Belarusian Travel';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG988.8' FROM concept WHERE preferred_label='Maltese Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG989.4' FROM concept WHERE preferred_label='Maltese Travel';

-- Iceland already has a dedicated travel subject and exact schedule range.
-- Remove its redundant permissive range, which incorrectly swallowed the
-- later antiquities/culture/ethnography schedule through DL334.
DELETE FROM lcc_range
WHERE concept_id=14021 AND class_letters='DL'
  AND start_value='301' AND end_value='334';

COMMIT;
