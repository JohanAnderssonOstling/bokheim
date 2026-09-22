BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Portuguese Social & Cultural History'), ('Portuguese Ethnography'),
  ('Portuguese Abroad'), ('Portuguese Political & Diplomatic History'),
  ('Portuguese Republic, 1910–Present'), ('Portugal, 1919–1974'),
  ('Democratic Portugal, 1974–Present'), ('António de Oliveira Salazar'),
  ('Porto History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3611,0 FROM concept WHERE preferred_label IN (
 'Portuguese Social & Cultural History','Portuguese Ethnography',
 'Portuguese Political & Diplomatic History','Porto History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Portuguese Ethnography'),0
FROM concept WHERE preferred_label='Portuguese Abroad';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11580,0 FROM concept WHERE preferred_label='Portuguese Republic, 1910–Present';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Portuguese Republic, 1910–Present'),0
FROM concept WHERE preferred_label IN (
 'Portugal, 1919–1974','Democratic Portugal, 1974–Present','António de Oliveira Salazar'
);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','532','532.7',concept_id FROM concept WHERE preferred_label='Portuguese Social & Cultural History';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','533','534.5.Z',concept_id FROM concept WHERE preferred_label='Portuguese Ethnography';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','556','556.8.Z',concept_id FROM concept WHERE preferred_label='Portuguese Political & Diplomatic History';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','675','682.5',concept_id FROM concept WHERE preferred_label='Portuguese Republic, 1910–Present';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','680','680.5.Z',concept_id FROM concept WHERE preferred_label='Portugal, 1919–1974';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DP','681','682.5.Z',concept_id FROM concept WHERE preferred_label='Democratic Portugal, 1974–Present';

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DP534.5' FROM concept WHERE preferred_label='Portuguese Abroad';
INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DP676.S25' FROM concept WHERE preferred_label='António de Oliveira Salazar';
INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DP802.O6' FROM concept WHERE preferred_label='Porto History';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (2471,'DP516'),(2471,'DP702.A28'),(2471,'DP702.A85'),
 (2471,'DP702.C414'),(2471,'DP702.E49'),(2471,'DP702.M15'),(2471,'DP757');

COMMIT;
