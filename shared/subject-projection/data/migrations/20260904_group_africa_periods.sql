-- Group Africa's direct chronological entries under one browse node, in
-- chronological display order.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id, preferred_label)
VALUES ('bokheim:history-lcc:681917eb-ee85-5c0f-bb72-29fe06702cf3', 'By Period');

INSERT INTO concept_parent(
    concept_id, parent_concept_id, ordinal, display_order
) VALUES (
    'bokheim:history-lcc:681917eb-ee85-5c0f-bb72-29fe06702cf3',
    'bokheim:subject:history:africa', 0, 20
);

UPDATE concept_parent
SET parent_concept_id = 'bokheim:history-lcc:681917eb-ee85-5c0f-bb72-29fe06702cf3',
    display_order = CASE concept_id
        WHEN 'bokheim:history-lcc-full:ddf3c156-a762-547e-8ea4-33d6364d5643' THEN 0
        WHEN 'bokheim:history-lcc-full:a3d1db0f-a0de-5d4a-93c4-e8af9f8bf250' THEN 10
        WHEN 'bokheim:history-lcc-full:c520085b-a0ce-5ede-9aeb-ccf42e26fd42' THEN 20
        WHEN 'bokheim:history-lcc-full:414685a8-b734-5875-b937-631b102f96ca' THEN 30
        WHEN 'bokheim:history-lcc-full:fef155c7-c037-5d07-bd22-7c1282e3abda' THEN 40
        WHEN 'bokheim:history-lcc-full:1f276b25-6ad6-515b-b355-0dae1b9c9038' THEN 50
        WHEN 'bokheim:history-lcc-full:ad52fcaf-bae9-5b6a-a992-20974a75465f' THEN 60
        WHEN 'bokheim:history-lcc-full:a66fb14b-c002-597f-8b98-f8ced42a6ea6' THEN 70
    END
WHERE parent_concept_id = 'bokheim:subject:history:africa'
  AND concept_id IN (
    'bokheim:history-lcc-full:ddf3c156-a762-547e-8ea4-33d6364d5643',
    'bokheim:history-lcc-full:a3d1db0f-a0de-5d4a-93c4-e8af9f8bf250',
    'bokheim:history-lcc-full:c520085b-a0ce-5ede-9aeb-ccf42e26fd42',
    'bokheim:history-lcc-full:414685a8-b734-5875-b937-631b102f96ca',
    'bokheim:history-lcc-full:fef155c7-c037-5d07-bd22-7c1282e3abda',
    'bokheim:history-lcc-full:1f276b25-6ad6-515b-b355-0dae1b9c9038',
    'bokheim:history-lcc-full:ad52fcaf-bae9-5b6a-a992-20974a75465f',
    'bokheim:history-lcc-full:a66fb14b-c002-597f-8b98-f8ced42a6ea6'
  );

COMMIT;
