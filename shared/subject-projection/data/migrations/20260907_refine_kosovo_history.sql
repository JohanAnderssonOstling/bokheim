BEGIN;

WITH wanted(label) AS (VALUES
 ('Kosovo Historical Reference'),('Kosovo Antiquities'),
 ('Kosovo Social & Cultural History'),('Kosovo Ethnography'),
 ('Kosovo Historical Biographies'),('Kosovo to 1389'),
 ('Ottoman Kosovo, 1389–1913'),('Kosovo, 1913–1945'),
 ('Autonomous Kosovo, 1945–2008'),('Independent Kosovo, 2008–Present'),
 ('Kosovo Travel'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51683,0 FROM concept WHERE preferred_label IN (
 'Kosovo Historical Reference','Kosovo Antiquities','Kosovo Social & Cultural History',
 'Kosovo Ethnography','Kosovo Historical Biographies','Kosovo to 1389',
 'Ottoman Kosovo, 1389–1913','Kosovo, 1913–1945',
 'Autonomous Kosovo, 1945–2008','Independent Kosovo, 2008–Present');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Kosovo Travel';

DELETE FROM lcc_range WHERE concept_id=51683 AND class_letters='DR';
INSERT INTO lcc_range SELECT 'DR','2075','2077',concept_id FROM concept WHERE preferred_label='Kosovo Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DR2078' FROM concept WHERE preferred_label='Kosovo Antiquities';
INSERT INTO lcc_selector SELECT concept_id,'DR2079' FROM concept WHERE preferred_label='Kosovo Social & Cultural History';
INSERT INTO lcc_range SELECT 'DR','2080','2080.5.Z',concept_id FROM concept WHERE preferred_label='Kosovo Ethnography';
INSERT INTO lcc_selector SELECT concept_id,'DR2081' FROM concept WHERE preferred_label='Kosovo Historical Biographies';
INSERT INTO lcc_selector SELECT concept_id,'DR2083' FROM concept WHERE preferred_label='Kosovo to 1389';
INSERT INTO lcc_range SELECT 'DR','2084','2084.5.Z',concept_id FROM concept WHERE preferred_label='Ottoman Kosovo, 1389–1913';
INSERT INTO lcc_range SELECT 'DR','2085','2085.5.Z',concept_id FROM concept WHERE preferred_label='Kosovo, 1913–1945';
-- Keep military events flattened into their broad chronological era.
INSERT INTO lcc_range SELECT 'DR','2086','2087.7',concept_id FROM concept WHERE preferred_label='Autonomous Kosovo, 1945–2008';
INSERT INTO lcc_range SELECT 'DR','2088','2088.2.Z',concept_id FROM concept WHERE preferred_label='Independent Kosovo, 2008–Present';

-- The explicit description-and-travel code overrides the reference block.
INSERT INTO lcc_selector SELECT concept_id,'DR2077' FROM concept WHERE preferred_label='Kosovo Travel';

COMMIT;
