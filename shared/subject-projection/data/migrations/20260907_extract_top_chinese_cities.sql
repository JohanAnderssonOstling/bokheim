BEGIN;

UPDATE concept SET preferred_label='Other Chinese Cities & Local Regions'
WHERE concept_id=50839;

WITH wanted(label) AS (VALUES
 ('Beijing History'),('Shanghai History'),('Hong Kong History'),
 ('Nanjing History'),('Macau History'),('Guangzhou History'),
 ('Tianjin History'),('Chongqing History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

-- These are siblings of the residual locality leaf. Making them descendants
-- of it would turn a permissive broad-range leaf into a permissive non-leaf.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3493,0 FROM concept WHERE preferred_label IN (
 'Beijing History','Shanghai History','Hong Kong History','Nanjing History',
 'Macau History','Guangzhou History','Tianjin History','Chongqing History');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','795','795.95',concept_id FROM concept WHERE preferred_label='Beijing History';
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Beijing History')
WHERE class_letters='DS' AND start_value='795' AND end_value='795.95';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','796.S2','796.S295',concept_id FROM concept WHERE preferred_label='Shanghai History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','796.H7','796.H795',concept_id FROM concept WHERE preferred_label='Hong Kong History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS796.N2' FROM concept WHERE preferred_label='Nanjing History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS797.56.N365' FROM concept WHERE preferred_label='Nanjing History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','796.M2','796.M295',concept_id FROM concept WHERE preferred_label='Macau History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS797.32.G836' FROM concept WHERE preferred_label='Guangzhou History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','796.T5','796.T595',concept_id FROM concept WHERE preferred_label='Tianjin History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','796.C592','796.C59295',concept_id FROM concept WHERE preferred_label='Chongqing History';

COMMIT;
