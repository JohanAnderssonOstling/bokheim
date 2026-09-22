-- Complete the official major-division partition of the QD schedule.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53336,'Chemistry Reference, History, Study & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53336,4803,0);

DELETE FROM lcc_range
WHERE concept_id=4803
  AND start_code='QD1'
  AND end_code='QD999';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QD1','QD65',53336);
