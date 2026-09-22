-- Make Pre-Columbian America a leaf and remove the outdated suffix from its
-- curated display label. Full descendant detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE pre_columbian_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id
 FROM concept_parent child
 WHERE child.parent_concept_id='unified:3cb2b26b-6996-558c-bff4-ebd8129997b0'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO pre_columbian_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM pre_columbian_detail)
   OR parent_concept_id IN (SELECT concept_id FROM pre_columbian_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM pre_columbian_detail);

UPDATE concept SET preferred_label='Pre-Columbian America'
WHERE concept_id='unified:3cb2b26b-6996-558c-bff4-ebd8129997b0';

DROP TABLE pre_columbian_detail;
COMMIT;
