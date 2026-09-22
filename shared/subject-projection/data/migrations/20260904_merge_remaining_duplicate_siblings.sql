-- Reconcile remaining same-label sibling concepts in the curated graph. Merge
-- source evidence and descendants into one retained concept per sibling pair.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE duplicate_map(
    loser TEXT PRIMARY KEY,
    winner TEXT NOT NULL
) WITHOUT ROWID;
INSERT INTO duplicate_map VALUES
 ('unified:de6fed92-081a-5d73-9db4-4ddbbf0eedee','unified:4887dbea-a685-5e73-8fdb-c776eb56ae3c'),
 ('bokheim:history-lcc:65f77d79-51b7-584f-9c73-d842052271e5','unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb'),
 ('unified:dfa81c71-041b-5218-aa04-2c85135ef522','unified:11d5232c-6d62-5af4-916f-a34c9f114002'),
 ('unified:57521598-0d64-579c-9d36-f74e14673fcc','unified:78f2df3b-9bf9-5051-884f-2f1e953d6011'),
 ('bokheim:history-lcc:6ae41ac2-c7b5-590f-b79d-c231de3bb95b','unified:91078d34-3022-5d7c-8648-876773a498eb'),
 ('bokheim:history-lcc:355dcc6e-e8fb-5175-a016-423f0d334ec4','unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232'),
 ('bokheim:history-lcc:31620a55-dde4-5581-bed5-baacdd131b66','unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9'),
 ('bokheim:history-lcc:a4aece6c-1599-5a3b-8085-0f23205f6c34','unified:179eaf78-59ba-588d-b0c6-3714dc8a7b02'),
 ('bokheim:history-lcc:e9a4bca9-1166-51ca-a544-12c165a467cd','unified:f5b0ca27-a635-5814-bcb4-3e6435a48615'),
 ('bokheim:history-lcc:3d1887f8-6d68-50a9-b0b7-fb7f2db27bca','unified:165c5a62-7543-55eb-8123-d9652b7a09a6'),
 ('bokheim:history-lcc:fa019425-ac49-5835-aedf-3a7bffecae71','unified:b36a5980-669c-5f3e-ac06-3a7f7a207b49'),
 ('bokheim:history-lcc:816749e3-2bf8-5fd2-bd47-264ac98e75ef','unified:0e2e95c7-6b98-5252-92ac-58be604c00e4'),
 ('bokheim:history-lcc:ad35eb64-2b14-5f7f-8b99-68ab15e087c9','bokheim:history-lcc:e3588f4f-1076-5c73-b0cc-723c971352a0'),
 ('bokheim:history-lcc:91546f0e-e1af-5033-895d-afa99a8c6cea','bokheim:history-lcc:c8b94a49-f008-56aa-a01a-2f60320beb78');

INSERT INTO source_label(concept_id,label,ordinal)
SELECT winner,label,base + row_number() OVER (PARTITION BY winner ORDER BY label)-1
FROM (
 SELECT map.winner,source.label,
        COALESCE((SELECT max(ordinal)+1 FROM source_label WHERE concept_id=map.winner),0) AS base
 FROM duplicate_map map JOIN source_label source ON source.concept_id=map.loser
 WHERE NOT EXISTS (SELECT 1 FROM source_label existing WHERE existing.concept_id=map.winner AND existing.label=source.label)
);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT winner,system_id,selector,base + row_number() OVER (PARTITION BY winner,system_id ORDER BY selector)-1
FROM (
 SELECT map.winner,source.system_id,source.selector,
        COALESCE((SELECT max(ordinal)+1 FROM source_selector WHERE concept_id=map.winner AND system_id=source.system_id),0) AS base
 FROM duplicate_map map JOIN source_selector source ON source.concept_id=map.loser
 WHERE NOT EXISTS (SELECT 1 FROM source_selector existing WHERE existing.concept_id=map.winner AND existing.system_id=source.system_id AND existing.selector=source.selector)
);

-- Retain any distinct parents and children owned only by the duplicate.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT winner,parent_concept_id,base + row_number() OVER (PARTITION BY winner ORDER BY parent_concept_id)-1,display_order
FROM (
 SELECT map.winner,edge.parent_concept_id,edge.display_order,
        COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=map.winner),0) AS base
 FROM duplicate_map map JOIN concept_parent edge ON edge.concept_id=map.loser
 WHERE NOT EXISTS (SELECT 1 FROM concept_parent existing WHERE existing.concept_id=map.winner AND existing.parent_concept_id=edge.parent_concept_id)
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,winner,base + row_number() OVER (PARTITION BY concept_id ORDER BY winner)-1,display_order
FROM (
 SELECT edge.concept_id,map.winner,edge.display_order,
        COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=edge.concept_id),0) AS base
 FROM duplicate_map map JOIN concept_parent edge ON edge.parent_concept_id=map.loser
 WHERE edge.concept_id<>map.winner
   AND NOT EXISTS (SELECT 1 FROM concept_parent existing WHERE existing.concept_id=edge.concept_id AND existing.parent_concept_id=map.winner)
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT loser FROM duplicate_map)
   OR parent_concept_id IN (SELECT loser FROM duplicate_map);
DELETE FROM concept WHERE concept_id IN (SELECT loser FROM duplicate_map);
DROP TABLE duplicate_map;
COMMIT;
