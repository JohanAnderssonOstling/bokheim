BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=290 AND start_code='TA349' AND end_code='TA359';

DELETE FROM lcc_range
WHERE concept_id=290 AND start_code='TA349' AND end_code='TA359';

INSERT INTO concept(preferred_label)
SELECT 'Applied Mechanics Fundamentals & Reference'
WHERE NOT EXISTS (
    SELECT 1 FROM concept
    WHERE preferred_label='Applied Mechanics Fundamentals & Reference'
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,290,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE parent_concept_id=290),0)
FROM concept c
WHERE c.preferred_label='Applied Mechanics Fundamentals & Reference'
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent cp
      WHERE cp.concept_id=c.concept_id AND cp.parent_concept_id=290
  );

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TA349','TA350.999',concept_id
FROM concept
WHERE preferred_label='Applied Mechanics Fundamentals & Reference';

COMMIT;
