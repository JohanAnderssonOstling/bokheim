BEGIN;

INSERT INTO concept(concept_id,preferred_label)
VALUES (51185,'African American Regional History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (51185,3378,0);

DELETE FROM lcc_range
WHERE concept_id=3378 AND class_letters='E'
  AND start_value='184.4' AND end_value='185.98';

-- General African American works remain directly on the parent. Geographic
-- works form a broad leaf; biography remains on the existing E185.96-E185.98
-- biography subject, and E185.2-E185.8999 belongs to the new topical leaves.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','184.4','185.1999',3378),
  ('E','185.9','185.9599',51185);

COMMIT;
