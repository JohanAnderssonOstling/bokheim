-- Replace the last TA range on the broad Civil navigation concept with a
-- specific polyhierarchical subject.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53380,'Engineering Acoustics');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53380,5569,0),
  (53380,5932,1);

DELETE FROM lcc_range
WHERE concept_id=5564
  AND start_code='TA365'
  AND end_code='TA367.5';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('TA365','TA367.5',53380);
