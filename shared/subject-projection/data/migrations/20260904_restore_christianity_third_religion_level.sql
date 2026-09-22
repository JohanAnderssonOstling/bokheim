-- Permit the Christianity branch to reach depth three below Religion. This
-- restores exactly the grandchildren of Christianity from master; restored
-- concepts are leaves in curated.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE christianity_child(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO christianity_child
SELECT edge.concept_id
FROM master.concept_parent edge
WHERE edge.parent_concept_id='unified:2c4b22db-f725-5427-a930-210f2de6642c';

CREATE TEMP TABLE christianity_grandchild(
 concept_id TEXT NOT NULL,
 parent_id TEXT NOT NULL,
 display_order INTEGER NOT NULL,
 PRIMARY KEY(concept_id,parent_id)
) WITHOUT ROWID;
INSERT INTO christianity_grandchild
SELECT edge.concept_id,edge.parent_concept_id,edge.display_order
FROM master.concept_parent edge
JOIN christianity_child parent ON parent.concept_id=edge.parent_concept_id;

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
SELECT source.concept_id,source.preferred_label
FROM master.concept source
JOIN christianity_grandchild restored ON restored.concept_id=source.concept_id;

INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT source.concept_id,source.label,source.ordinal
FROM master.source_label source
JOIN christianity_grandchild restored ON restored.concept_id=source.concept_id;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT source.concept_id,source.system_id,source.selector,source.ordinal
FROM master.source_selector source
JOIN christianity_grandchild restored ON restored.concept_id=source.concept_id;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT restored.concept_id,restored.parent_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=restored.concept_id),0)
       + row_number() OVER (PARTITION BY restored.concept_id ORDER BY restored.parent_id)-1,
       restored.display_order
FROM christianity_grandchild restored
WHERE restored.parent_id IN (SELECT concept_id FROM concept);

DROP TABLE christianity_grandchild;
DROP TABLE christianity_child;
COMMIT;
DETACH DATABASE master;
