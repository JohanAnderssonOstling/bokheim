-- Simplify the curated Canada branch. Population detail remains available in
-- the master taxonomy; period entries retain their meaning without generic
-- "By period" wrapper nodes.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE canada_population_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE subtree(concept_id) AS (
    VALUES ('bokheim:history-lcc-full:853f086f-3711-59bf-a6af-3b1683e87b57')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO canada_population_detail SELECT concept_id FROM subtree;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM canada_population_detail)
   OR parent_concept_id IN (SELECT concept_id FROM canada_population_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM canada_population_detail);

CREATE TEMP TABLE canada_period(
    concept_id TEXT PRIMARY KEY,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO canada_period
SELECT concept_id, display_order
FROM concept_parent
WHERE parent_concept_id='bokheim:history-lcc-full:96b4957b-348f-58ed-8f9b-e7c2fde5f30d';

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM canada_period)
  AND parent_concept_id='bokheim:history-lcc-full:96b4957b-348f-58ed-8f9b-e7c2fde5f30d';

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       'unified:704e7f13-642a-5dd0-a2f0-56c3dc29161e',
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=period.concept_id), 0),
       1000 + period.display_order
FROM canada_period period;

DELETE FROM concept_parent
WHERE concept_id='bokheim:history-lcc-full:96b4957b-348f-58ed-8f9b-e7c2fde5f30d'
   OR parent_concept_id='bokheim:history-lcc-full:96b4957b-348f-58ed-8f9b-e7c2fde5f30d';
DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:96b4957b-348f-58ed-8f9b-e7c2fde5f30d';

CREATE TEMP TABLE canada_social_period(
    concept_id TEXT PRIMARY KEY,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO canada_social_period
SELECT concept_id, display_order
FROM concept_parent
WHERE parent_concept_id='bokheim:history-lcc-full:d0765493-7f4f-5893-8790-daffbc7ad6d6';

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM canada_social_period)
  AND parent_concept_id='bokheim:history-lcc-full:d0765493-7f4f-5893-8790-daffbc7ad6d6';

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       'bokheim:history-lcc-full:abdb3cc9-795d-548b-843f-fb9c9a52f9a3',
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=period.concept_id), 0),
       period.display_order
FROM canada_social_period period;

DELETE FROM concept_parent
WHERE concept_id='bokheim:history-lcc-full:d0765493-7f4f-5893-8790-daffbc7ad6d6'
   OR parent_concept_id='bokheim:history-lcc-full:d0765493-7f4f-5893-8790-daffbc7ad6d6';
DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:d0765493-7f4f-5893-8790-daffbc7ad6d6';

DROP TABLE canada_social_period;
DROP TABLE canada_period;
DROP TABLE canada_population_detail;
COMMIT;
