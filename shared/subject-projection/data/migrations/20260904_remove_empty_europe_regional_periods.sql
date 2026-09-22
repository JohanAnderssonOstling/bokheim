-- Remove empty national-period duplicates from European regional period trees.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_wrapper(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO period_wrapper
SELECT p.concept_id FROM concept r JOIN concept_parent p ON p.parent_concept_id=r.concept_id JOIN concept c ON c.concept_id=p.concept_id
WHERE r.preferred_label IN ('Central Europe','Eastern Europe','Northern Europe','History of Balkan Peninsula') AND lower(c.preferred_label)='by period';

CREATE TEMP TABLE period_tree(wrapper_id TEXT,concept_id TEXT,PRIMARY KEY(wrapper_id,concept_id)) WITHOUT ROWID;
WITH RECURSIVE t(wrapper_id,id) AS (SELECT concept_id,concept_id FROM period_wrapper UNION SELECT t.wrapper_id,p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id)
INSERT INTO period_tree SELECT wrapper_id,id FROM t;

CREATE TEMP TABLE empty_period(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO empty_period
SELECT n.concept_id FROM period_tree n
WHERE n.concept_id<>n.wrapper_id
AND NOT EXISTS (
 WITH RECURSIVE sub(id) AS (VALUES(n.concept_id) UNION SELECT p.concept_id FROM sub JOIN concept_parent p ON p.parent_concept_id=sub.id)
 SELECT 1 FROM sub JOIN source_selector s ON s.concept_id=sub.id
);

-- Only delete nodes that are not also used outside these regional period trees.
CREATE TEMP TABLE deletable_period(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO deletable_period
SELECT e.concept_id FROM empty_period e
WHERE NOT EXISTS (
 SELECT 1 FROM concept_parent p WHERE p.concept_id=e.concept_id
 AND p.parent_concept_id NOT IN (SELECT concept_id FROM period_tree)
);

DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM deletable_period) OR parent_concept_id IN (SELECT concept_id FROM deletable_period);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM deletable_period);

DROP TABLE deletable_period; DROP TABLE empty_period; DROP TABLE period_tree; DROP TABLE period_wrapper;
COMMIT;
