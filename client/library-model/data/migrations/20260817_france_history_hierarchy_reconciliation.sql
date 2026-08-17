-- Reconcile the mixed outline/MDSConnect hierarchy beneath France.
--
-- The outline already supplied several French eras directly beneath France,
-- while the one-level structural expansion also produced a redundant History
-- node, a second By period node, and children flattened from several different
-- period records.  Keep one visible period hierarchy and preserve every useful
-- LCC selector on its surviving concept.

BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
    ('bokheim:france-history:revolutionary-napoleonic-period','Revolutionary and Napoleonic Period, 1789–1815'),
    ('bokheim:france-history:21st-century','21st Century');

INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES
    ('bokheim:france-history:revolutionary-napoleonic-period','Revolutionary and Napoleonic period, 1789-1815',0),
    ('bokheim:france-history:21st-century','21st century',0);

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
    ('bokheim:france-history:revolutionary-napoleonic-period','lcc','DC139..DC249@4',0),
    ('bokheim:france-history:21st-century','lcc','DC425..DC433@4',0),
    ('unified:0f5b44eb-7daa-5e3f-bc80-6f6e2b4ffd52','lcc','DC60..DC433@4',1),
    ('unified:0f5b44eb-7daa-5e3f-bc80-6f6e2b4ffd52','lcc','DC60..DC424@3',2),
    ('unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f','lcc','DC111..DC120@4',1),
    ('unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143','lcc','DC35..DC41@3',3),
    ('unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143','lcc','DC35..DC424@2',4),
    ('unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143','lcc','DC44..DC59.8@3',5);

-- The context-free source caption "By period" at DC161-DC219 is meaningful
-- only inside the Revolutionary and Napoleonic period.  Preserve that source
-- label but use a self-contained visible label.
UPDATE concept
SET preferred_label='Revolutionary Governments and First Empire, 1787–1815'
WHERE concept_id='bokheim:lcc-detail:e3ede418-d61b-558d-abdd-ab1771d1626b';

-- France has one canonical period umbrella.
UPDATE concept_parent
SET parent_concept_id='unified:0f5b44eb-7daa-5e3f-bc80-6f6e2b4ffd52', display_order=10
WHERE concept_id='unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';
UPDATE concept_parent
SET parent_concept_id='unified:0f5b44eb-7daa-5e3f-bc80-6f6e2b4ffd52', display_order=20
WHERE concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
    ('bokheim:france-history:revolutionary-napoleonic-period','unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2',0,40),
    ('bokheim:france-history:21st-century','unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2',0,70);

-- Nest previously flat eras beneath their actual chronological umbrellas.
UPDATE concept_parent
SET parent_concept_id='unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a', display_order=20
WHERE concept_id='unified:e2f5915d-cc6e-54eb-8cf9-f1ac1fd1ba34'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';
UPDATE concept_parent
SET parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2', display_order=10
WHERE concept_id='unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';
UPDATE concept_parent
SET parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2', display_order=50
WHERE concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';
UPDATE concept_parent
SET parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2', display_order=60
WHERE concept_id='unified:1a6ed314-e0f9-5d5a-8202-b2830634b7b4'
  AND parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143';

-- Children flattened from the Revolutionary/Napoleonic snapshot record.
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:revolutionary-napoleonic-period', display_order=20
WHERE concept_id='bokheim:lcc-detail:f3657345-dab7-5de0-838d-3a5c1f51083f' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:revolutionary-napoleonic-period', display_order=30
WHERE concept_id='bokheim:lcc-detail:e3ede418-d61b-558d-abdd-ab1771d1626b' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:revolutionary-napoleonic-period', display_order=40
WHERE concept_id='bokheim:lcc-detail:af38ae37-b692-5e25-aac2-89d29df0864c' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:revolutionary-napoleonic-period', display_order=50
WHERE concept_id='bokheim:lcc-detail:4a7a8ea6-1fd4-5b4f-8b4a-e721f5488216' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';

-- Children flattened from the 19th-century snapshot record.
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=10
WHERE concept_id='bokheim:lcc-detail:767ac943-0603-58af-ad4f-58fbc9bd7c2e' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=20
WHERE concept_id='bokheim:lcc-detail:973fa435-545d-58cc-b49b-1f0d42553bce' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=30
WHERE concept_id='bokheim:lcc-detail:c9030a57-37cf-5731-805f-7f03599fcd30' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=40
WHERE concept_id='bokheim:lcc-detail:7d3b2024-54c4-52b0-8962-d7d6a0296a0f' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=50
WHERE concept_id='bokheim:lcc-detail:465f5016-3882-5644-a8e6-753f460b1af2' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=60
WHERE concept_id='bokheim:lcc-detail:ea2940eb-0f9e-54ca-9e23-9824e95763f0' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=70
WHERE concept_id='bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54', display_order=80
WHERE concept_id='bokheim:lcc-detail:720f0809-8b10-53d4-b530-7e21c2a75f88' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';

-- Children flattened from the 21st-century snapshot record.
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:21st-century', display_order=10
WHERE concept_id='bokheim:lcc-detail:b842f8f1-8ed8-5231-84b4-c51ac0e4a2c4' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:21st-century', display_order=20
WHERE concept_id='bokheim:lcc-detail:e85d76fe-df4d-5fb3-83f1-4b426a978e60' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:21st-century', display_order=30
WHERE concept_id='bokheim:lcc-detail:7f596f9f-0762-5c69-b95f-87bbc70a53ae' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
UPDATE concept_parent SET parent_concept_id='bokheim:france-history:21st-century', display_order=40
WHERE concept_id='bokheim:lcc-detail:4677ca24-51af-56fb-afaf-ef4f7aec1e1c' AND parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';

-- General French historiography is not a period; retain it directly beneath
-- France before removing the redundant France > History umbrella.
UPDATE concept_parent
SET parent_concept_id='unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143', display_order=70
WHERE concept_id='bokheim:lcc-detail:47db1b19-00bf-5a72-bce1-9687f1960358'
  AND parent_concept_id='unified:8e705874-0a07-523d-a3f6-0eb0f2c85342';

-- Remove exact duplicates after transferring their detailed selectors.
DELETE FROM concept_parent
WHERE concept_id IN (
        'bokheim:lcc-detail:b5f05fa8-f2d0-5745-9e7e-ac175bb7944b',
        'bokheim:lcc-detail:7393d58f-2bfb-531b-859a-764ac7d667d8',
        'unified:8e705874-0a07-523d-a3f6-0eb0f2c85342'
    )
   OR parent_concept_id IN (
        'bokheim:lcc-detail:b5f05fa8-f2d0-5745-9e7e-ac175bb7944b',
        'bokheim:lcc-detail:7393d58f-2bfb-531b-859a-764ac7d667d8',
        'unified:8e705874-0a07-523d-a3f6-0eb0f2c85342'
    );
DELETE FROM source_selector WHERE concept_id IN (
    'bokheim:lcc-detail:b5f05fa8-f2d0-5745-9e7e-ac175bb7944b',
    'bokheim:lcc-detail:7393d58f-2bfb-531b-859a-764ac7d667d8',
    'unified:8e705874-0a07-523d-a3f6-0eb0f2c85342'
);
DELETE FROM source_label WHERE concept_id IN (
    'bokheim:lcc-detail:b5f05fa8-f2d0-5745-9e7e-ac175bb7944b',
    'bokheim:lcc-detail:7393d58f-2bfb-531b-859a-764ac7d667d8',
    'unified:8e705874-0a07-523d-a3f6-0eb0f2c85342'
);
DELETE FROM concept WHERE concept_id='bokheim:lcc-detail:b5f05fa8-f2d0-5745-9e7e-ac175bb7944b';
DELETE FROM concept WHERE concept_id='bokheim:lcc-detail:7393d58f-2bfb-531b-859a-764ac7d667d8';
DELETE FROM concept WHERE concept_id='unified:8e705874-0a07-523d-a3f6-0eb0f2c85342';

COMMIT;
