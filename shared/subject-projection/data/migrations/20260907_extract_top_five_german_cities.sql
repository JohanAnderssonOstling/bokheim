BEGIN;

WITH wanted(label) AS (VALUES
 ('Hamburg History'),('Munich History'),('Frankfurt am Main History'),
 ('Cologne History'),('Nuremberg History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13904,0 FROM concept WHERE preferred_label IN (
 'Hamburg History','Munich History','Frankfurt am Main History',
 'Cologne History','Nuremberg History');

-- Official LCC city blocks within DD901, kept as ranges rather than expanded
-- into many individual selectors.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DD','901.H2','901.H29',concept_id FROM concept WHERE preferred_label='Hamburg History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DD','901.M71','901.M95',concept_id FROM concept WHERE preferred_label='Munich History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DD','901.F7','901.F79',concept_id FROM concept WHERE preferred_label='Frankfurt am Main History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DD','901.C7','901.C79',concept_id FROM concept WHERE preferred_label='Cologne History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DD','901.N91','901.N92',concept_id FROM concept WHERE preferred_label='Nuremberg History';

COMMIT;
