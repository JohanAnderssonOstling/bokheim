-- Restore the German source range that spans the late 19th and 20th
-- centuries; it cannot accurately be labeled 20th Century alone.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
VALUES (54910,'19th–20th Century');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 54910,54909,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=54909),-1)+1;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (54910,'PT2600-PT2653');

COMMIT;
