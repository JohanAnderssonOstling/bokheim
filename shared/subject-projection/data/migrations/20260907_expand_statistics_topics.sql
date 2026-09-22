BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Sampling & Survey Methodology'),
 ('Statistical Data Processing');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12952,0 FROM concept
WHERE preferred_label IN ('Sampling & Survey Methodology','Statistical Data Processing');

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Sampling & Survey Methodology')
WHERE concept_id=5520 AND selector='HA31.2';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Statistical Data Processing')
WHERE concept_id=5520 AND selector='HA32';

UPDATE lcc_selector SET concept_id=12952
WHERE concept_id=5520 AND selector IN
 ('HA29-HA29.5','HA29-HA32','HA29.6','HA30','HA30.3','HA30.4','HA30.6',
  'HA31','HA31.15','HA31.23','HA31.25','HA31.3','HA31.35','HA31.38',
  'HA31.4','HA31.5','HA31.7','HA31.9');

UPDATE lcc_selector SET concept_id=12953
WHERE concept_id=5520 AND selector IN ('HA36','HA36-HA37');

UPDATE lcc_selector SET concept_id=12954
WHERE concept_id=5520 AND selector='HA38-HA39';

UPDATE lcc_selector SET concept_id=12955
WHERE concept_id=5520 AND selector='HA154-HA155';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5520,'HA1-HA4737');

DELETE FROM lcc_range
WHERE concept_id=12951 AND start_code='HA1' AND end_code='HA4737';

COMMIT;
