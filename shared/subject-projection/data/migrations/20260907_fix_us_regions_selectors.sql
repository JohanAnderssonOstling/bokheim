BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51271,'U.S. Territories'),
  (51272,'U.S. Insular Possessions');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51271,3418,0),(51272,3418,0);

UPDATE lcc_selector SET concept_id=51271 WHERE concept_id=3418 AND selector='F965';
UPDATE lcc_selector SET concept_id=51272 WHERE concept_id=3418 AND selector='F970';

-- KGV and KGZ are law subclasses. They do not belong in regional history.
DELETE FROM lcc_selector WHERE concept_id=3418 AND selector IN ('KGV','KGZ');
DELETE FROM lcc_range WHERE concept_id=3418 AND class_letters IN ('KGV','KGZ');

COMMIT;
