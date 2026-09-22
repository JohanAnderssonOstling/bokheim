BEGIN;

UPDATE concept SET preferred_label='Social Classes' WHERE concept_id=5073;

-- Preserve the literal broad notation without allowing the parent to absorb
-- every specific HT classification.
DELETE FROM lcc_range
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='601' AND end_value='1445';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5073,'HT601-1445');

WITH wanted(label) AS (VALUES
 ('Origins of Social Classes'),
 ('Birth-Based Classes'),
 ('Nobility & Aristocracy'),
 ('Commons'),
 ('Occupational Classes'),
 ('Middle Classes'),
 ('Caste Systems'),
 ('Serfdom'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5073,0 FROM concept WHERE preferred_label IN (
 'Origins of Social Classes','Birth-Based Classes','Nobility & Aristocracy',
 'Commons','Occupational Classes','Middle Classes','Caste Systems','Serfdom');

-- Slavery is already a substantive curated subject. Add it to this branch
-- rather than making a duplicate.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 12240,5073,COALESCE(MAX(ordinal),-1)+1
FROM concept_parent WHERE concept_id=12240
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent
    WHERE concept_id=12240 AND parent_concept_id=5073
  );

UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Origins of Social Classes')
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='621' AND end_value='635';

-- Remove the obsolete truncated duplicate before moving the complete span.
DELETE FROM lcc_range
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='641' AND end_value='657';
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Birth-Based Classes')
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='641' AND end_value='658.22';

DELETE FROM lcc_range
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='675' AND end_value='690';
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Occupational Classes')
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='675' AND end_value='690.2';

UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Caste Systems')
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='713' AND end_value='725';
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Serfdom')
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='751' AND end_value='815';
UPDATE lcc_range SET concept_id=12240
WHERE concept_id=5073 AND class_letters='HT'
  AND start_value='851' AND end_value='1445';

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HT','647','653',concept_id FROM concept
WHERE preferred_label='Nobility & Aristocracy';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HT','655','657.22',concept_id FROM concept
WHERE preferred_label='Commons';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HT','680','690',concept_id FROM concept
WHERE preferred_label='Middle Classes';

COMMIT;
