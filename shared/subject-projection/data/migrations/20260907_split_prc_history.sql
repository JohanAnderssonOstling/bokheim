BEGIN;

UPDATE concept SET preferred_label='China, 1949–1976' WHERE concept_id=10421;
UPDATE concept SET preferred_label='China, 2002–Present' WHERE concept_id=10587;

WITH wanted(label) AS (VALUES
 ('China, 1976–2002'),
 ('Chinese Social & Cultural History, 1949–1976'),
 ('Chinese Political History, 1949–1976'),
 ('Chinese Foreign Relations, 1949–1976'),
 ('Chinese Biographies, 1949–1976'),
 ('Cultural Revolution, 1966–1976'),
 ('Chinese Social & Cultural History, 1976–2002'),
 ('Chinese Political History, 1976–2002'),
 ('Chinese Foreign Relations, 1976–2002'),
 ('Chinese Biographies, 1976–2002'),
 ('Mao Zedong'),('Deng Xiaoping'),('Zhou Enlai'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3493,0 FROM concept WHERE preferred_label='China, 1976–2002';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,10421,0 FROM concept WHERE preferred_label IN (
 'Chinese Social & Cultural History, 1949–1976',
 'Chinese Political History, 1949–1976','Chinese Foreign Relations, 1949–1976',
 'Chinese Biographies, 1949–1976','Cultural Revolution, 1966–1976',
 'Mao Zedong','Deng Xiaoping','Zhou Enlai');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,p.concept_id,0 FROM concept c JOIN concept p
WHERE p.preferred_label='China, 1976–2002' AND c.preferred_label IN (
 'Chinese Social & Cultural History, 1976–2002',
 'Chinese Political History, 1976–2002','Chinese Foreign Relations, 1976–2002',
 'Chinese Biographies, 1976–2002');

-- Diplomatic subjects are also reachable through the established thematic arm.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51024,1 FROM concept WHERE preferred_label IN
 ('Chinese Foreign Relations, 1949–1976','Chinese Foreign Relations, 1976–2002');

-- Individual people are also biographies. Deng spans both chronological eras.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13354,1 FROM concept WHERE preferred_label IN
 ('Mao Zedong','Zhou Enlai');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13354,2 FROM concept WHERE preferred_label='Deng Xiaoping';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,p.concept_id,1 FROM concept c JOIN concept p
WHERE c.preferred_label='Deng Xiaoping' AND p.preferred_label='China, 1976–2002';

-- Remove the old overlapping permissive PRC ranges. Period parents retain
-- exact general, reference, military, and unextracted-event codes only.
DELETE FROM lcc_range WHERE concept_id=10421 AND class_letters='DS';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 10421,v FROM (
 SELECT 'DS777.545' v UNION ALL SELECT 'DS777.546' UNION ALL SELECT 'DS777.547'
 UNION ALL SELECT 'DS777.548' UNION ALL SELECT 'DS777.549'
 UNION ALL SELECT 'DS777.55' UNION ALL SELECT 'DS777.56'
 UNION ALL SELECT 'DS777.58' UNION ALL SELECT 'DS777.65'
 UNION ALL SELECT 'DS777.7' UNION ALL SELECT 'DS778.4'
 UNION ALL SELECT 'DS778.5' UNION ALL SELECT 'DS778.6');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS777.6' FROM concept WHERE preferred_label='Chinese Social & Cultural History, 1949–1976';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS777.75' FROM concept WHERE preferred_label='Chinese Political History, 1949–1976';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS777.8' FROM concept WHERE preferred_label='Chinese Foreign Relations, 1949–1976';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','778.A1','778.Z',concept_id FROM concept WHERE preferred_label='Chinese Biographies, 1949–1976';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS778.7' FROM concept WHERE preferred_label='Cultural Revolution, 1966–1976';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS778.M3' FROM concept WHERE preferred_label='Mao Zedong';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS778.D46' FROM concept WHERE preferred_label='Deng Xiaoping';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS778.C593' FROM concept WHERE preferred_label='Zhou Enlai';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,v FROM concept c CROSS JOIN (
 SELECT 'DS779.15' v UNION ALL SELECT 'DS779.16' UNION ALL SELECT 'DS779.17'
 UNION ALL SELECT 'DS779.18' UNION ALL SELECT 'DS779.19'
 UNION ALL SELECT 'DS779.2' UNION ALL SELECT 'DS779.215'
 UNION ALL SELECT 'DS779.22' UNION ALL SELECT 'DS779.24'
 UNION ALL SELECT 'DS779.25' UNION ALL SELECT 'DS779.3'
 UNION ALL SELECT 'DS779.32')
WHERE c.preferred_label='China, 1976–2002';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.23' FROM concept WHERE preferred_label='Chinese Social & Cultural History, 1976–2002';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.26' FROM concept WHERE preferred_label='Chinese Political History, 1976–2002';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS779.27' FROM concept WHERE preferred_label='Chinese Foreign Relations, 1976–2002';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','779.28','779.29.Z',concept_id FROM concept WHERE preferred_label='Chinese Biographies, 1976–2002';

COMMIT;
