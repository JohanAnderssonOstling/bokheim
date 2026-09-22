BEGIN;

UPDATE concept SET preferred_label='Ethnic Groups & Races' WHERE concept_id=5060;

-- Preserve literal broad catalogue notations, not permissive parent ranges.
DELETE FROM lcc_range WHERE concept_id=5060 AND class_letters='GN';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5060,'GN537-674'),
 (5060,'GN550-674');

WITH wanted(label) AS (VALUES
 ('Cross-Regional Ethnic Groups'),
 ('North American Ethnic Groups'),
 ('Latin American Ethnic Groups'),
 ('European Ethnic Groups'),
 ('Asian Ethnic Groups'),
 ('African Ethnic Groups'),
 ('Australian & Pacific Ethnic Groups'),
 ('Arctic & Tropical Ethnic Groups'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5060,0 FROM concept WHERE preferred_label IN (
 'Cross-Regional Ethnic Groups','North American Ethnic Groups',
 'Latin American Ethnic Groups','European Ethnic Groups',
 'Asian Ethnic Groups','African Ethnic Groups',
 'Australian & Pacific Ethnic Groups','Arctic & Tropical Ethnic Groups');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','537','549',concept_id FROM concept
WHERE preferred_label='Cross-Regional Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','550','560',concept_id FROM concept
WHERE preferred_label='North American Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','562','564',concept_id FROM concept
WHERE preferred_label='Latin American Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','575','585',concept_id FROM concept
WHERE preferred_label='European Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','625','635',concept_id FROM concept
WHERE preferred_label='Asian Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','643','661',concept_id FROM concept
WHERE preferred_label='African Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','662','671',concept_id FROM concept
WHERE preferred_label='Australian & Pacific Ethnic Groups';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'GN','673','674',concept_id FROM concept
WHERE preferred_label='Arctic & Tropical Ethnic Groups';

COMMIT;
