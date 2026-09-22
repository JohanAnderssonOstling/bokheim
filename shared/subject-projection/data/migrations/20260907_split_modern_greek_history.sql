BEGIN;

UPDATE concept SET preferred_label='Modern Greek History' WHERE concept_id=3568;
UPDATE concept SET preferred_label='Greece, 1821–1913' WHERE concept_id=51546;
UPDATE concept SET preferred_label='Greece since 1913' WHERE concept_id=51547;

WITH wanted(label) AS (VALUES
 ('Modern Greek Social & Cultural History'),
 ('Aegean Islands History'),
 ('Greek Historical Geography & Monuments'),
 ('Modern Greek Political & Military History'),
 ('Greek Diaspora History'),
 ('Modern Greek Historical Reference & Biography'),
 ('Greece, 1830–1913'),
 ('Greece, 1913–1967'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3568,0 FROM concept WHERE preferred_label IN (
 'Modern Greek Social & Cultural History','Aegean Islands History',
 'Greek Historical Geography & Monuments','Modern Greek Political & Military History',
 'Greek Diaspora History','Modern Greek Historical Reference & Biography');

-- Reuse the existing Ottoman-period concept rather than introducing a
-- duplicate beneath Modern Greek History.
DELETE FROM concept_parent WHERE concept_id=10223;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(10223,3568,0);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51546,0 FROM concept WHERE preferred_label='Greece, 1830–1913';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,51547,0 FROM concept
WHERE preferred_label='Greece, 1913–1967';

-- Make the established event and regime ranges children of their enclosing
-- chronological periods instead of overlapping siblings.
DELETE FROM concept_parent WHERE concept_id IN (10160,10480) AND parent_concept_id=3568;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(10160,51546,0),(10480,51547,0);

-- Nonleafs retain only exact broad/general codes; their permissive ranges are
-- replaced by mutually exclusive leaf ranges.
DELETE FROM lcc_range WHERE concept_id IN (3568,51546,51547);
DELETE FROM lcc_range WHERE concept_id IN (
 SELECT concept_id FROM concept WHERE preferred_label='Greece, 1913–1967');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','745','747',concept_id FROM concept
WHERE preferred_label='Modern Greek Social & Cultural History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','895','900.999',concept_id FROM concept
WHERE preferred_label='Aegean Islands History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','750','755.82',concept_id FROM concept
WHERE preferred_label='Modern Greek Historical Reference & Biography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','765','787',concept_id FROM concept
WHERE preferred_label='Modern Greek Political & Military History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','816','832',concept_id FROM concept WHERE preferred_label='Greece, 1830–1913';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','833.001','849.499',concept_id FROM concept WHERE preferred_label='Greece, 1913–1967';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DF','849.581','852.999',concept_id FROM concept WHERE preferred_label='Greece, 1913–1967';
UPDATE lcc_range SET start_value='853.501'
WHERE concept_id=51549 AND class_letters='DF' AND start_value='854' AND end_value='854.32';

-- Broad history numbers remain exact on the Modern Greek umbrella.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 3568,v FROM (
 SELECT 'DF757' v UNION ALL SELECT 'DF758' UNION ALL SELECT 'DF759'
 UNION ALL SELECT 'DF760' UNION ALL SELECT 'DF802');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES(51546,'DF803');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES(51547,'DF833');

UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Modern Greek Social & Cultural History')
WHERE concept_id=3568 AND selector='DF741';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Aegean Islands History')
WHERE concept_id=3568 AND selector='DF895';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Greek Historical Geography & Monuments')
WHERE concept_id=3568 AND selector IN ('DF715','DF719','DF720');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Modern Greek Political & Military History')
WHERE concept_id=3568 AND selector IN ('DF807','DF809');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Greek Diaspora History')
WHERE concept_id=3568 AND selector='DF748';

-- Clean up concepts created by an earlier application of this migration before
-- the existing Ottoman and restored-democracy subjects were detected.
DELETE FROM concept_parent WHERE concept_id IN (
 SELECT concept_id FROM concept WHERE preferred_label IN
 ('Greece under Ottoman Rule, 1453–1821','Greece, 1974–Present'));
DELETE FROM concept WHERE preferred_label IN
 ('Greece under Ottoman Rule, 1453–1821','Greece, 1974–Present');

COMMIT;
