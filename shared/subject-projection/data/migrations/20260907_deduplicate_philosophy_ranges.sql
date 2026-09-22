BEGIN;

-- Remove a range only when a strictly broader range on the same Philosophy
-- concept and in the same LCC class already contains it. Coverage and subject
-- ownership therefore remain unchanged.
WITH RECURSIVE philosophy(concept_id) AS (
  SELECT 4487
  UNION
  SELECT cp.concept_id
  FROM concept_parent cp JOIN philosophy p ON p.concept_id=cp.parent_concept_id
), redundant(class_letters,start_value,end_value,concept_id) AS (
  SELECT narrow.class_letters,narrow.start_value,narrow.end_value,narrow.concept_id
  FROM lcc_range narrow
  JOIN philosophy p ON p.concept_id=narrow.concept_id
  WHERE EXISTS (
    SELECT 1 FROM lcc_range broad
    WHERE broad.concept_id=narrow.concept_id
      AND broad.class_letters=narrow.class_letters
      AND CAST(broad.start_value AS REAL)<=CAST(narrow.start_value AS REAL)
      AND CAST(broad.end_value AS REAL)>=CAST(narrow.end_value AS REAL)
      AND (CAST(broad.start_value AS REAL)<CAST(narrow.start_value AS REAL)
        OR CAST(broad.end_value AS REAL)>CAST(narrow.end_value AS REAL))
  )
)
DELETE FROM lcc_range
WHERE (class_letters,start_value,end_value,concept_id) IN
      (SELECT class_letters,start_value,end_value,concept_id FROM redundant);

COMMIT;
