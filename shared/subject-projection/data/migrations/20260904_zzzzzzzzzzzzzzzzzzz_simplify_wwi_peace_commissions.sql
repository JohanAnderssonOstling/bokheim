-- Collapse the personnel/region filing wrappers beneath World War I peace.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='United States Peace Efforts'
WHERE concept_id='bokheim:history-lcc-full:9eb98ae9-cc4c-54c3-846b-015c61247dec';

UPDATE concept_parent
SET parent_concept_id='bokheim:lcc-detail:0aec2ea1-d10b-5863-966b-f5c35ffc1181',
    display_order=10
WHERE concept_id='bokheim:history-lcc-full:9eb98ae9-cc4c-54c3-846b-015c61247dec'
  AND parent_concept_id='bokheim:history-lcc-full:3fe1b77d-fd92-5118-8ef8-98e800f18872';

WITH moved AS (
    SELECT s.system_id,s.selector,
           row_number() OVER (PARTITION BY s.system_id ORDER BY s.concept_id,s.ordinal) AS offset
    FROM source_selector s
    WHERE s.concept_id IN (
        'bokheim:history-lcc-full:08f7c879-e54f-5da2-8188-9a9836360b24',
        'bokheim:history-lcc-full:3fe1b77d-fd92-5118-8ef8-98e800f18872'
    )
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:lcc-detail:0aec2ea1-d10b-5863-966b-f5c35ffc1181',
       moved.system_id,moved.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='bokheim:lcc-detail:0aec2ea1-d10b-5863-966b-f5c35ffc1181'
                   AND existing.system_id=moved.system_id),0)
       + moved.offset - 1
FROM moved;

DELETE FROM concept_parent
WHERE concept_id IN (
    'bokheim:history-lcc-full:08f7c879-e54f-5da2-8188-9a9836360b24',
    'bokheim:history-lcc-full:3fe1b77d-fd92-5118-8ef8-98e800f18872'
)
OR parent_concept_id IN (
    'bokheim:history-lcc-full:08f7c879-e54f-5da2-8188-9a9836360b24',
    'bokheim:history-lcc-full:3fe1b77d-fd92-5118-8ef8-98e800f18872'
);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:history-lcc-full:08f7c879-e54f-5da2-8188-9a9836360b24',
    'bokheim:history-lcc-full:3fe1b77d-fd92-5118-8ef8-98e800f18872'
);

COMMIT;
