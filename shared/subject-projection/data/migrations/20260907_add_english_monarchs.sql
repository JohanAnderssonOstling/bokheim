BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES (51454,'English Monarchs');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51454,3566,0),(51454,13353,1);

-- DA28 as a whole is collective national biography. The royal subdivision is
-- DA28.1 through DA28.3; DA28.35 onward covers noble families and other people.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES('DA','28.1','28.349',51454);

COMMIT;
