-- Restore the selector-bearing period that became orphaned during earlier
-- history flattening, using its short curated label.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id,preferred_label)
VALUES ('bokheim:history-lcc-full:6beca34c-5423-5633-bd3c-3700276e0861', 'Western Empire, 364–476');

INSERT INTO source_label(concept_id,label,ordinal)
SELECT concept_id,label,ordinal FROM master.source_label
WHERE concept_id='bokheim:history-lcc-full:6beca34c-5423-5633-bd3c-3700276e0861';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,system_id,selector,ordinal FROM master.source_selector
WHERE concept_id='bokheim:history-lcc-full:6beca34c-5423-5633-bd3c-3700276e0861';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT edge.concept_id,edge.parent_concept_id,edge.ordinal,edge.display_order
FROM master.concept_parent edge
JOIN concept parent ON parent.concept_id=edge.parent_concept_id
WHERE edge.concept_id='bokheim:history-lcc-full:6beca34c-5423-5633-bd3c-3700276e0861';

COMMIT;
DETACH DATABASE master;
