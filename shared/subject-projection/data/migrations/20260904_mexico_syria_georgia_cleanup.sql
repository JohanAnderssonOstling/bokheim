-- Additional conservative regional cleanup in the curated taxonomy. Full
-- detail remains in the master database.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE regional_detail_to_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;

-- Mexico: remove the Cities subtree.
WITH RECURSIVE subtree(concept_id) AS (
    VALUES ('bokheim:lcc-detail:fa2a532c-f2b8-59d4-bd30-45a9a88851e1')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT OR IGNORE INTO regional_detail_to_remove SELECT concept_id FROM subtree;

-- Georgia: remove the remaining direct county entries and their detail.
WITH RECURSIVE subtree(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    JOIN concept item ON item.concept_id=child.concept_id
    WHERE child.parent_concept_id='unified:9427e868-ecaa-52fb-9bfd-8d6d6e4b1257'
      AND item.preferred_label LIKE '% County'
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT OR IGNORE INTO regional_detail_to_remove SELECT concept_id FROM subtree;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM regional_detail_to_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove);

DROP TABLE regional_detail_to_remove;
COMMIT;
