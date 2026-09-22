-- Remove remaining generic publication-form branches and all Antiquities
-- variants throughout the curated taxonomy. Biography remains retained.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE removed_label(label TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_label VALUES
 ('societies'),
 ('periodicals, societies, etc.'),
 ('periodicals'),
 ('periodicals, societies, congresses, serial publications'),
 ('yearbooks'),
 ('collected works'),
 ('handbooks, manuals, etc.'),
 ('dictionaries'),
 ('exhibitions'),
 ('research'),
 ('encyclopedias. dictionaries'),
 ('museums. exhibitions');

CREATE TEMP TABLE removed_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept.concept_id
 FROM concept
 LEFT JOIN removed_label label ON label.label=lower(trim(concept.preferred_label))
 WHERE label.label IS NOT NULL
    OR lower(trim(concept.preferred_label)) LIKE 'antiquities%'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO removed_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM removed_detail)
   OR parent_concept_id IN (SELECT concept_id FROM removed_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_detail);

DROP TABLE removed_detail;
DROP TABLE removed_label;
COMMIT;
