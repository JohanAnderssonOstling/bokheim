BEGIN;

-- Preserve the unsplittable broad notation as an exact parent selector.  The
-- permissive numeric coverage is assigned to LCC-aligned leaves below.
DELETE FROM lcc_range
WHERE concept_id=5463
  AND class_letters='HV'
  AND start_value='6001'
  AND end_value='7220.5';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5463,'HV6001-7220.5');

INSERT INTO concept(preferred_label)
SELECT 'Criminal Anthropology'
WHERE NOT EXISTS (
  SELECT 1 FROM concept WHERE preferred_label='Criminal Anthropology'
);
INSERT INTO concept(preferred_label)
SELECT 'Criminal Classes'
WHERE NOT EXISTS (
  SELECT 1 FROM concept WHERE preferred_label='Criminal Classes'
);
INSERT INTO concept(preferred_label)
SELECT 'Crimes & Offenses'
WHERE NOT EXISTS (
  SELECT 1 FROM concept WHERE preferred_label='Crimes & Offenses'
);
INSERT INTO concept(preferred_label)
SELECT 'Crime by Region or Country'
WHERE NOT EXISTS (
  SELECT 1 FROM concept WHERE preferred_label='Crime by Region or Country'
);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5463,0 FROM concept WHERE preferred_label='Criminal Anthropology';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5463,0 FROM concept WHERE preferred_label='Criminal Classes';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5463,0 FROM concept WHERE preferred_label='Crimes & Offenses';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5463,0 FROM concept WHERE preferred_label='Crime by Region or Country';

-- Rehome the two existing outline ranges that were still attached to the
-- Criminology parent, and add the remaining official first-level spans.
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Criminal Anthropology')
WHERE concept_id=5463 AND class_letters='HV'
  AND start_value='6035' AND end_value='6197';
UPDATE lcc_range
SET concept_id=12727
WHERE concept_id=5463 AND class_letters='HV'
  AND start_value='6250' AND end_value='6250.4';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','6201','6249.2',concept_id FROM concept
WHERE preferred_label='Criminal Classes';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','6250.92','6773.42',concept_id FROM concept
WHERE preferred_label='Crimes & Offenses';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','6773.92','7220.52',concept_id FROM concept
WHERE preferred_label='Crime by Region or Country';

-- These exact codes are child-abuse classifications, not general criminology.
UPDATE lcc_selector SET concept_id=13275
WHERE concept_id=5463 AND selector IN ('HV6626.23.C2','HV6626.54');

-- Remove the selectorless artificial abuse wrapper while retaining its four
-- substantive children directly under Criminology.
UPDATE concept_parent
SET parent_concept_id=5463
WHERE parent_concept_id=12728
  AND concept_id IN (13273,13274,13275,13276);
DELETE FROM concept_parent WHERE concept_id=12728 OR parent_concept_id=12728;
DELETE FROM source_label WHERE concept_id=12728;
DELETE FROM bisac_selector WHERE concept_id=12728;
DELETE FROM lcc_selector WHERE concept_id=12728;
DELETE FROM lcc_range WHERE concept_id=12728;
DELETE FROM concept WHERE concept_id=12728;

COMMIT;
