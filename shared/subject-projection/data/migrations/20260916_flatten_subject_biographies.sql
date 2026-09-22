-- Flatten subject-specific biography wrappers into Biography.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE subject_biography_candidates AS
WITH RECURSIVE ancestors(concept_id,ancestor_id) AS (
  SELECT concept_id,concept_id FROM concept
  UNION ALL
  SELECT a.concept_id,cp.parent_concept_id
  FROM ancestors a JOIN concept_parent cp ON cp.concept_id=a.ancestor_id
)
SELECT c.concept_id,
       CASE
         WHEN c.preferred_label LIKE '% Biographies'
           THEN rtrim(substr(c.preferred_label,1,length(c.preferred_label)-12))
         ELSE rtrim(substr(c.preferred_label,1,length(c.preferred_label)-10))
       END AS new_label
FROM concept c
WHERE (c.preferred_label LIKE '% Biographies' OR c.preferred_label LIKE '% Biography')
  AND c.preferred_label NOT LIKE 'Biography%'
  AND c.preferred_label NOT LIKE 'YA %'
  AND c.preferred_label NOT LIKE '% Biography &%'
  AND NOT EXISTS (
    SELECT 1 FROM ancestors a
    WHERE a.concept_id=c.concept_id AND a.ancestor_id IN (3212,51001)
  );

UPDATE concept
SET preferred_label=(SELECT new_label FROM subject_biography_candidates b WHERE b.concept_id=concept.concept_id)
WHERE concept_id IN (SELECT concept_id FROM subject_biography_candidates);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM subject_biography_candidates);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3212,ROW_NUMBER() OVER (ORDER BY new_label,concept_id)-1
FROM subject_biography_candidates;

DROP TABLE subject_biography_candidates;
COMMIT;
