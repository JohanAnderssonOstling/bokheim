BEGIN;

INSERT INTO concept(preferred_label)
SELECT 'Romance Philology & Reference'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Romance Philology & Reference');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3823,0 FROM concept WHERE preferred_label='Romance Philology & Reference';

DELETE FROM lcc_range WHERE concept_id=3823;
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'PC','1','599.999',concept_id FROM concept
WHERE preferred_label='Romance Philology & Reference';

COMMIT;
