-- Collapse exact national literature year bands into century periods.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_merges(exact_id INTEGER PRIMARY KEY, century_id INTEGER NOT NULL);
INSERT INTO period_merges VALUES
 (3913,4179),(3914,4179),(3916,4180),
 (4246,4191),(12494,4218),(12495,4218),(12496,4219),
 (12704,4190),(12705,4190),(12716,4228),(12717,4229);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT pm.century_id,ls.selector
FROM period_merges pm JOIN lcc_selector ls ON ls.concept_id=pm.exact_id;

CREATE TEMP TABLE child_merges AS
SELECT cp.concept_id AS child_id,pm.century_id
FROM concept_parent cp JOIN period_merges pm ON pm.exact_id=cp.parent_concept_id;

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT exact_id FROM period_merges);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,century_id,
       COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=child_merges.century_id),-1)
       + ROW_NUMBER() OVER (PARTITION BY century_id ORDER BY child_id)
FROM child_merges;

DELETE FROM lcc_selector WHERE concept_id IN (SELECT exact_id FROM period_merges);
DELETE FROM concept WHERE concept_id IN (SELECT exact_id FROM period_merges);

DROP TABLE child_merges;
DROP TABLE period_merges;

COMMIT;
