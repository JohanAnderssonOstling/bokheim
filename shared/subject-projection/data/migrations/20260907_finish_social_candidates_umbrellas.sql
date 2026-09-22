BEGIN;

DELETE FROM lcc_range WHERE concept_id=5083;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5083,'HF1-6182'),(5083,'HF1351-1647'),(5083,'HF54.5-54.56');
-- Specific commerce spans go to real children instead of the umbrella.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('HF1351','HF1647',5087);
INSERT INTO concept(preferred_label) SELECT 'Commerce Information Services'
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Commerce Information Services');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5083,0 FROM concept WHERE preferred_label='Commerce Information Services';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HF54.5','HF54.56',concept_id FROM concept WHERE preferred_label='Commerce Information Services';

DELETE FROM lcc_range WHERE concept_id=5381;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (5381,'BF1-990');

DELETE FROM lcc_range WHERE concept_id=5369;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5369,'JA1-26'),(5369,'JA1-92'),(5369,'JA27-34'),
 (5369,'JA60-64'),(5369,'JA71.5-72.5');
UPDATE lcc_selector SET concept_id=12597 WHERE concept_id=5369 AND selector='JA74.5';
UPDATE lcc_selector SET concept_id=12598 WHERE concept_id=5369 AND selector='JA75.7';
UPDATE lcc_selector SET concept_id=12599 WHERE concept_id=5369 AND selector='JA76';
UPDATE lcc_selector SET concept_id=12600 WHERE concept_id=5369 AND selector='JA79';
UPDATE lcc_selector SET concept_id=12601 WHERE concept_id=5369 AND selector IN ('JA81','JA83');
UPDATE lcc_selector SET concept_id=12603 WHERE concept_id=5369 AND selector='JA86';

WITH wanted(label) AS (VALUES ('Political-Science Reference'),('Quantitative Political Methods'))
INSERT INTO concept(preferred_label) SELECT label FROM wanted
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label=label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5369,0 FROM concept WHERE preferred_label IN ('Political-Science Reference','Quantitative Political Methods');
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'JA1','JA69',concept_id FROM concept WHERE preferred_label='Political-Science Reference';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'JA71.5','JA72.5',concept_id FROM concept WHERE preferred_label='Quantitative Political Methods';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JA81','JA84',12601),('JA85','JA85.2',12602),('JA86','JA88',12603);

COMMIT;
