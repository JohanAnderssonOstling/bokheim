BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50831,'Reference & Collected Works'),
  (50832,'Historiography'),
  (50833,'General Works on Civilization');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50831,3622,0),
  (50832,3622,0),
  (50833,3622,0);

-- All enumerated parent selectors except the broad CB class and the genuinely
-- isolated study, special, by-period, and Early Christian entries are covered
-- by the two official spans below.
DELETE FROM source_selector
WHERE concept_id=3622 AND system_id='lcc'
  AND selector NOT IN ('CB','CB20','CB151','CB304.2','CB331');

UPDATE lcc_range SET concept_id=50832
WHERE class_letters='CB' AND start_value='15' AND end_value='18';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('CB','3','13',50831),
  ('CB','23','113.Z',50833);

COMMIT;
