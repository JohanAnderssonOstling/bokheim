BEGIN;

UPDATE concept
SET preferred_label='Republican China, 1912–1949'
WHERE concept_id=10425;

WITH wanted(label) AS (VALUES
 ('China, 1912–1928'),
 ('China, 1928–1937'),
 ('China, 1937–1945'),
 ('China, 1945–1949'),
 ('Republican Chinese Social & Cultural History'),
 ('Republican Chinese Political History'),
 ('Republican Chinese Foreign Relations'),
 ('Republican Chinese Biographies'),
 ('Chiang Kai-shek'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,10425,0 FROM concept WHERE preferred_label IN (
 'China, 1912–1928','China, 1928–1937','China, 1937–1945','China, 1945–1949',
 'Republican Chinese Social & Cultural History','Republican Chinese Political History',
 'Republican Chinese Foreign Relations','Republican Chinese Biographies');

-- Foreign relations and individual biography remain reachable through the
-- established thematic branches as well as through Republican Chinese history.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51024,1 FROM concept
WHERE preferred_label='Republican Chinese Foreign Relations';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,p.concept_id,0 FROM concept c JOIN concept p
WHERE c.preferred_label='Chiang Kai-shek' AND p.preferred_label='China, 1928–1937';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13354,1 FROM concept WHERE preferred_label='Chiang Kai-shek';

-- The umbrella is non-permissive. Its broad reference and unsplit thematic
-- numbers remain exact; chronological leaves own the mutually exclusive ranges.
DELETE FROM lcc_range WHERE concept_id=10425 AND class_letters='DS';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 10425,v FROM (
 SELECT 'DS773.83' v UNION ALL SELECT 'DS773.86' UNION ALL SELECT 'DS773.89'
 UNION ALL SELECT 'DS773.92' UNION ALL SELECT 'DS773.94' UNION ALL SELECT 'DS774'
 UNION ALL SELECT 'DS774.5' UNION ALL SELECT 'DS774.7'
 UNION ALL SELECT 'DS775.4' UNION ALL SELECT 'DS775.5');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','776.4','777.462',concept_id FROM concept WHERE preferred_label='China, 1912–1928';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','777.47','777.514',concept_id FROM concept WHERE preferred_label='China, 1928–1937';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','777.518','777.534',concept_id FROM concept WHERE preferred_label='China, 1937–1945';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','777.535','777.544',concept_id FROM concept WHERE preferred_label='China, 1945–1949';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS775.2' FROM concept
WHERE preferred_label='Republican Chinese Social & Cultural History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS775.7' FROM concept
WHERE preferred_label='Republican Chinese Political History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS775.8' FROM concept
WHERE preferred_label='Republican Chinese Foreign Relations';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,v FROM concept c CROSS JOIN (
 SELECT 'DS776' v UNION ALL SELECT 'DS777')
WHERE c.preferred_label='Republican Chinese Biographies';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS777.488.C5' FROM concept WHERE preferred_label='Chiang Kai-shek';

COMMIT;
