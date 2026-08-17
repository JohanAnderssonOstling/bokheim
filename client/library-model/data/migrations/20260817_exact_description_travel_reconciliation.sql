-- Reconcile History-side Description and travel concepts that have one exact
-- existing destination below Geography, Places & Travel / Travel.

BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
    ('unified:b9425b8d-85cf-5d63-978c-50e463c296b9','lcc','DH31..DH40@2',0),
    ('unified:fc73baac-cbfe-55d5-9dd0-87aa4824dfea','lcc','DL115..DL120@4',0),
    ('unified:d71eab94-5a11-527a-9ed2-2b0b6c37f206','lcc','DL1015..DL1015.4@4',0),
    ('unified:d5f495e9-235e-5969-8b1a-95dbb4abd101','lcc','DD21.5..DD43@2',0),
    ('unified:3069a831-0888-54cd-bc1c-702cd13db6be','lcc','DA969..DA969@4',0),
    ('unified:404cb2c5-3395-5ad7-a1d2-285a9d7294bd','lcc','DS807..DS812@4',0),
    ('unified:58e91b87-287b-5957-a6d6-3076a8c906dd','lcc','DS902.2..DS902.4@4',0),
    ('unified:4f758744-1249-585c-a602-be52977093e4','lcc','DL415..DL419.2@4',0),
    ('unified:b989125a-16eb-53b3-9896-f4decb28d820','lcc','DL614.55..DL619.5@4',0),
    ('unified:73b2652a-f8e2-5881-956f-5c755dc6c075','lcc','DQ20.5..DQ26@2',0),
    ('unified:7b1ec571-b6ee-5d1f-b053-9acaffcff1fd','lcc','DR421..DR429.4@4',0);

UPDATE concept_parent
SET parent_concept_id=CASE parent_concept_id
    WHEN 'unified:607f91da-4e44-5720-9785-3413afeb0877' THEN 'unified:b9425b8d-85cf-5d63-978c-50e463c296b9'
    WHEN 'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5' THEN 'unified:fc73baac-cbfe-55d5-9dd0-87aa4824dfea'
    WHEN 'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f' THEN 'unified:d71eab94-5a11-527a-9ed2-2b0b6c37f206'
    WHEN 'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0' THEN 'unified:d5f495e9-235e-5969-8b1a-95dbb4abd101'
    WHEN 'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f' THEN 'unified:3069a831-0888-54cd-bc1c-702cd13db6be'
    WHEN 'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed' THEN 'unified:404cb2c5-3395-5ad7-a1d2-285a9d7294bd'
    WHEN 'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8' THEN 'unified:58e91b87-287b-5957-a6d6-3076a8c906dd'
    WHEN 'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417' THEN 'unified:4f758744-1249-585c-a602-be52977093e4'
    WHEN 'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1' THEN 'unified:b989125a-16eb-53b3-9896-f4decb28d820'
    WHEN 'unified:3625e6a5-1d01-5958-9149-e86a274534f8' THEN 'unified:73b2652a-f8e2-5881-956f-5c755dc6c075'
    WHEN 'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00' THEN 'unified:7b1ec571-b6ee-5d1f-b053-9acaffcff1fd'
END
WHERE parent_concept_id IN (
    'unified:607f91da-4e44-5720-9785-3413afeb0877',
    'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5',
    'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f',
    'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0',
    'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f',
    'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed',
    'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8',
    'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417',
    'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1',
    'unified:3625e6a5-1d01-5958-9149-e86a274534f8',
    'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00'
);

DELETE FROM concept_parent WHERE concept_id IN (
    'unified:607f91da-4e44-5720-9785-3413afeb0877',
    'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5',
    'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f',
    'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0',
    'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f',
    'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed',
    'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8',
    'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417',
    'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1',
    'unified:3625e6a5-1d01-5958-9149-e86a274534f8',
    'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00'
);
DELETE FROM source_selector WHERE concept_id IN (
    'unified:607f91da-4e44-5720-9785-3413afeb0877',
    'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5',
    'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f',
    'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0',
    'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f',
    'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed',
    'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8',
    'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417',
    'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1',
    'unified:3625e6a5-1d01-5958-9149-e86a274534f8',
    'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00'
);
DELETE FROM source_label WHERE concept_id IN (
    'unified:607f91da-4e44-5720-9785-3413afeb0877',
    'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5',
    'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f',
    'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0',
    'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f',
    'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed',
    'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8',
    'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417',
    'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1',
    'unified:3625e6a5-1d01-5958-9149-e86a274534f8',
    'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00'
);
DELETE FROM concept WHERE concept_id IN (
    'unified:607f91da-4e44-5720-9785-3413afeb0877',
    'bokheim:lcc-detail:f064752e-4d1c-56b5-b1be-e76ddca987a5',
    'bokheim:lcc-detail:e59db4de-1df7-5db6-9029-907596444f2f',
    'unified:5b3d5fbf-3c62-52e4-bcdc-ab0217aa1be0',
    'bokheim:lcc-detail:61a957dd-ac48-5f71-ba0b-ae7e38c3848f',
    'bokheim:lcc-detail:7551b8c8-43f5-56cc-a0e5-5f2bc29114ed',
    'bokheim:lcc-detail:fd024ae4-d639-566d-a586-0d27e47280b8',
    'bokheim:lcc-detail:a9f4fb4a-bc2d-5355-8462-060118f2d417',
    'bokheim:lcc-detail:47faeec9-3e96-5b45-bdc6-350d79fbd0d1',
    'unified:3625e6a5-1d01-5958-9149-e86a274534f8',
    'bokheim:lcc-detail:6a338afe-5a9c-5dd2-85de-2c3a5728fc00'
);

COMMIT;
