BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES(50931,'Chicago');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(50931,3442,0);

DELETE FROM lcc_range
WHERE concept_id=3442 AND class_letters='F'
  AND CAST(start_value AS REAL)>=548 AND CAST(end_value AS REAL)<549;
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES('F','548','548.999',50931);

COMMIT;
