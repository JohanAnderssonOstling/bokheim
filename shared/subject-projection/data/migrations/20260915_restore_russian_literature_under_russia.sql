-- Russian literature's period and form subjects belong under the BISAC Russia
-- node, rather than directly under Slavic & Eurasian.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id IN (12490,12491,12492,12493,12494,12495,12496,12497);

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal) VALUES
  (12490,4216,0),(12491,4216,0),(12492,4216,0),(12493,4216,0),
  (12494,4216,0),(12495,4216,0),(12496,4216,0),(12497,4216,0);

COMMIT;
