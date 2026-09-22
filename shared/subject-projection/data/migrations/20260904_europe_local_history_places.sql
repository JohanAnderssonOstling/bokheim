-- Remove Local History & Places branches throughout curated European history.
-- The complete geographic detail remains available in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE europe_local_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
europe(concept_id) AS (
    VALUES ('bokheim:subject:history:europe')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN europe parent ON parent.concept_id=child.parent_concept_id
),
local_root(concept_id) AS (
    SELECT item.concept_id
    FROM europe item
    JOIN concept c ON c.concept_id=item.concept_id
    WHERE lower(c.preferred_label)='local history & places'
),
detail(concept_id) AS (
    SELECT concept_id FROM local_root
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO europe_local_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM europe_local_detail)
   OR parent_concept_id IN (SELECT concept_id FROM europe_local_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM europe_local_detail);

DROP TABLE europe_local_detail;
COMMIT;
