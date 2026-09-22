BEGIN;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (12554,13042,1);

UPDATE lcc_selector SET concept_id=13040
WHERE concept_id=9977 AND selector='BF442';

UPDATE lcc_selector SET concept_id=13041
WHERE concept_id=9977 AND selector IN ('BF443-BF443.5','BF443.4-BF443.5');

UPDATE lcc_selector SET concept_id=13042
WHERE concept_id=9977 AND selector IN
 ('BF444','BF444.5','BF445','BF446','BF447','BF449','BF449.5');

UPDATE lcc_selector SET concept_id=12554
WHERE concept_id=9977 AND selector='BF448';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (9977,'BF441-BF449.5');

DELETE FROM lcc_range
WHERE concept_id=9977 AND start_code='BF441' AND end_code='BF449.5';

COMMIT;
