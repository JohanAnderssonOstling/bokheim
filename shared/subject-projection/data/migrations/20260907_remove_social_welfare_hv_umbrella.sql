BEGIN;

DELETE FROM lcc_range WHERE concept_id=5462;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5462,'HV1-9960');

WITH wanted(label) AS (VALUES
 ('Social-Welfare Reference & Education'),
 ('History of Philanthropy'),
 ('Social-Welfare Biography & Information'),
 ('Charity & Government Policy'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5462,0 FROM concept WHERE preferred_label IN (
 'Social-Welfare Reference & Education','History of Philanthropy',
 'Social-Welfare Biography & Information');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5897,0 FROM concept WHERE preferred_label='Charity & Government Policy';

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HV1','HV15',concept_id FROM concept
WHERE preferred_label='Social-Welfare Reference & Education';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HV16','HV25',concept_id FROM concept
WHERE preferred_label='History of Philanthropy';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HV27','HV31',concept_id FROM concept
WHERE preferred_label='Social-Welfare Biography & Information';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('HV40','HV69.2',5467);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5467,'HV40-69.2');
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HV70','HV72',concept_id FROM concept
WHERE preferred_label='Charity & Government Policy';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'HV70-72' FROM concept
WHERE preferred_label='Charity & Government Policy';

COMMIT;
