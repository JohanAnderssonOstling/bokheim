BEGIN;

WITH wanted(label) AS (VALUES
 ('Chinese Social & Cultural History, 2002–Present'),
 ('Chinese Political History, 2002–Present'),
 ('Chinese Foreign Relations, 2002–Present'),
 ('Chinese Diaspora'),
 ('Chinese Historical Geography'),
 ('Chinese Historical Sites & Monuments'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,10587,0 FROM concept c WHERE c.preferred_label IN (
 'Chinese Social & Cultural History, 2002–Present',
 'Chinese Political History, 2002–Present',
 'Chinese Foreign Relations, 2002–Present');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51024,1 FROM concept
WHERE preferred_label='Chinese Foreign Relations, 2002–Present';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,50835,0 FROM concept WHERE preferred_label='Chinese Diaspora';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12446,1 FROM concept WHERE preferred_label='Chinese Diaspora';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3493,0 FROM concept
WHERE preferred_label IN ('Chinese Historical Geography','Chinese Historical Sites & Monuments');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13834,1 FROM concept
WHERE preferred_label='Chinese Historical Sites & Monuments';

-- Transfer each exact LCC number from the Chinese-history umbrella to its
-- specific destination. No ranges or general-work numbers are changed.
DELETE FROM lcc_selector
WHERE concept_id=3493 AND selector IN (
 'DS706.3','DS706.5','DS706.7','DS732',
 'DS779.43','DS779.44','DS779.45','DS779.46','DS779.47');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.43' FROM concept
WHERE preferred_label='Chinese Social & Cultural History, 2002–Present';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.46' FROM concept
WHERE preferred_label='Chinese Political History, 2002–Present';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.47' FROM concept
WHERE preferred_label='Chinese Foreign Relations, 2002–Present';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS732' FROM concept WHERE preferred_label='Chinese Diaspora';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,v FROM concept c CROSS JOIN (
 SELECT 'DS706.5' v UNION ALL SELECT 'DS706.7')
WHERE c.preferred_label='Chinese Historical Geography';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS706.3' FROM concept
WHERE preferred_label='Chinese Historical Sites & Monuments';

-- The unused military and naval numbers remain exact on the chronological
-- leaf rather than creating empty subjects.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 10587,v FROM (SELECT 'DS779.44' v UNION ALL SELECT 'DS779.45');

COMMIT;
