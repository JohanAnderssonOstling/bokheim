-- Restore the fifteen selector-bearing subjects that were mistakenly removed.
-- Only their visible preferred labels should be shortened.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE restored_subject(
 concept_id TEXT PRIMARY KEY,
 short_label TEXT NOT NULL
) WITHOUT ROWID;
INSERT INTO restored_subject VALUES
 ('bokheim:lcc-detail:1951db22-ee37-5fa4-8b6c-6fab63f6b79c', 'Atomic & Molecular Structure'),
 ('bokheim:lcc-detail:08f00646-6d0e-5188-932a-6fdd1e88c071', 'Hospital Medical Management'),
 ('bokheim:lcc-detail:e42d1337-16d3-5218-be73-e90c1da15991', 'Minority & Bilingual Education'),
 ('bokheim:lcc-detail:25cc440a-6baf-5289-9886-8fbeffc82ec4', 'Biotech Compounds'),
 ('bokheim:history-lcc-full:02ac7177-3b6a-5df8-9400-6b603cc2b00b', 'Yugoslavia, 1918–1945'),
 ('bokheim:lcc-detail:3cbdc8b8-c4a3-5aee-8c75-ab8b459d9f12', 'Placental & Gestational Disorders'),
 ('bokheim:lcc-detail:61247679-df63-5351-9fb0-9792668ed393', 'Magnetic Compass Deviation'),
 ('bokheim:lcc-detail:ce1456ae-e3a1-50c3-909b-b3b3a1bc70d9', 'UN Refugee Organizations'),
 ('bokheim:lcc-detail:c8aded4e-1c1a-5c4e-800b-e182dcd6e883', 'Medical Practice Regulation'),
 ('bokheim:lcc-detail:6ea4b05b-d4c0-5684-b173-e6789f15602f', 'Difference & Integral Equations'),
 ('bokheim:lcc-detail:b4684045-f503-5f72-a6ae-f7d6c2e7fd01', 'American Music Before 1860'),
 ('bokheim:lcc-detail:44b0106e-ae72-5bf1-b628-98b9fb3749c9', 'Operating Room Assistance'),
 ('bokheim:lcc-detail:e55dcf4f-8983-5c6c-899d-698c932051c4', 'Art in Wartime'),
 ('bokheim:lcc-detail:44f4ec8d-ae5e-544c-9aee-c3bc78d4c0d8', 'Industrial Engineering Profession'),
 ('bokheim:lcc-detail:deed514d-6aa4-5ded-9f7b-f1feb02341e4', 'Electronics Manufacturing');

-- Undo the prior transfer to the immediate parent.
DELETE FROM source_selector
WHERE EXISTS (
 SELECT 1
 FROM restored_subject restored
 JOIN master.source_selector original ON original.concept_id=restored.concept_id
 JOIN master.concept_parent edge ON edge.concept_id=restored.concept_id
 WHERE source_selector.concept_id=edge.parent_concept_id
   AND source_selector.system_id=original.system_id
   AND source_selector.selector=original.selector
);

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,short_label FROM restored_subject;

INSERT INTO source_label(concept_id,label,ordinal)
SELECT label.concept_id,label.label,label.ordinal
FROM master.source_label label
JOIN restored_subject restored ON restored.concept_id=label.concept_id;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT selector.concept_id,selector.system_id,selector.selector,selector.ordinal
FROM master.source_selector selector
JOIN restored_subject restored ON restored.concept_id=selector.concept_id;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT edge.concept_id,edge.parent_concept_id,edge.ordinal,edge.display_order
FROM master.concept_parent edge
JOIN restored_subject restored ON restored.concept_id=edge.concept_id
JOIN concept parent ON parent.concept_id=edge.parent_concept_id;

DROP TABLE restored_subject;
COMMIT;
DETACH DATABASE master;
