-- Keep World War II operational chronology, but remove the redundant
-- September 1939-December 1941 grouping layer.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='Military Operations'
WHERE concept_id='bokheim:history-lcc-full:6ac3acd0-42f6-57a4-a158-bc96eb8a4101';

UPDATE concept
SET preferred_label='September 1939–May 1940'
WHERE concept_id='bokheim:history-lcc-full:62219208-d12f-5d87-b865-9ae10d805aa8';

UPDATE concept_parent
SET parent_concept_id='bokheim:history-lcc-full:6ac3acd0-42f6-57a4-a158-bc96eb8a4101'
WHERE parent_concept_id='bokheim:history-lcc-full:b6eee934-fde1-5520-99ae-3a80e38aa254';

UPDATE concept_parent
SET display_order=CASE concept_id
    WHEN 'bokheim:history-lcc-full:62219208-d12f-5d87-b865-9ae10d805aa8' THEN 10
    WHEN 'bokheim:history-lcc-full:6ce990fe-5963-57cd-ac86-f4c9f5fa1f55' THEN 20
    WHEN 'bokheim:history-lcc-full:a8317c88-cc5a-5a8b-95c2-15ec52b564f6' THEN 30
    WHEN 'bokheim:history-lcc-full:dd854253-0caf-5d54-8696-f998f7df8e8c' THEN 40
    WHEN 'bokheim:history-lcc-full:fd000ce1-310d-579e-97c7-1bcd4226afe1' THEN 50
    WHEN 'bokheim:history-lcc-full:fd97dc78-93a4-5fdb-993c-814433eb5e00' THEN 60
    WHEN 'bokheim:history-lcc-full:81a6f2de-dd46-5cb1-ab22-0f78c1e84dc2' THEN 70
END
WHERE parent_concept_id='bokheim:history-lcc-full:6ac3acd0-42f6-57a4-a158-bc96eb8a4101';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:history-lcc-full:6ac3acd0-42f6-57a4-a158-bc96eb8a4101',
       system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='bokheim:history-lcc-full:6ac3acd0-42f6-57a4-a158-bc96eb8a4101'
                   AND existing.system_id=source_selector.system_id),0)
FROM source_selector
WHERE concept_id='bokheim:history-lcc-full:b6eee934-fde1-5520-99ae-3a80e38aa254';

DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:b6eee934-fde1-5520-99ae-3a80e38aa254';

COMMIT;
