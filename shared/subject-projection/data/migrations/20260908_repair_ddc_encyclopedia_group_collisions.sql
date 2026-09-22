PRAGMA foreign_keys=ON;
BEGIN;

INSERT INTO concept(preferred_label)
SELECT 'Germanic' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Germanic');
INSERT INTO concept(preferred_label)
SELECT 'Iberian' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Iberian');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,4705,0 FROM concept WHERE preferred_label IN('Germanic','Iberian');

DELETE FROM concept_parent WHERE concept_id IN(7804,9723,9795);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
 (7804,(SELECT concept_id FROM concept WHERE preferred_label='Germanic'),0),
 (9723,(SELECT concept_id FROM concept WHERE preferred_label='Germanic'),0),
 (9795,(SELECT concept_id FROM concept WHERE preferred_label='Iberian'),0);

INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
VALUES
 ('033',(SELECT concept_id FROM concept WHERE preferred_label='Germanic')),
 ('036',(SELECT concept_id FROM concept WHERE preferred_label='Iberian'));

COMMIT;
