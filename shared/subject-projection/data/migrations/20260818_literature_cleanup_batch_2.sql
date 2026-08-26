-- Continue Literature cleanup using semantic concepts rather than preserving
-- source-specific wrappers. All source selectors and detailed children are
-- retained. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Collapse two selector-free fiction groupings into Genres.
DELETE FROM concept_parent
WHERE parent_concept_id IN (
    'bokheim:fiction:history-conflict',
    'bokheim:fiction:places-ways-life'
)
  AND EXISTS (
      SELECT 1
      FROM concept_parent existing
      WHERE existing.concept_id = concept_parent.concept_id
        AND existing.parent_concept_id = 'bokheim:subject:literature:genres'
  );

UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:genres'
WHERE parent_concept_id IN (
    'bokheim:fiction:history-conflict',
    'bokheim:fiction:places-ways-life'
);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:fiction:history-conflict',
    'bokheim:fiction:places-ways-life'
);

-- Replace the legacy PZ wrapper with unified English- and juvenile-fiction
-- routes. The broad PZ fallback remains on Genres.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:subject:literature:genres', 'lcc', 'PZ', 0),
    ('bokheim:subject:literature:genres', 'lcc', 'PZ1..PZ90@1', 1),
    ('unified:3a171dff-53a4-5a2d-a008-666ee14809a2', 'lcc', 'PZ1..PZ4@2', 0);

UPDATE concept_parent
SET parent_concept_id = 'unified:3a171dff-53a4-5a2d-a008-666ee14809a2'
WHERE parent_concept_id = 'unified:904684fc-8248-5716-9c7b-6238589bbcbd';

DELETE FROM concept
WHERE concept_id = 'unified:904684fc-8248-5716-9c7b-6238589bbcbd';

UPDATE concept
SET preferred_label = 'By Language'
WHERE concept_id = 'unified:9605db13-9baa-5ca2-99e0-1e715fca1c18';

UPDATE concept_parent
SET parent_concept_id = 'unified:170b17af-be96-56f7-a9b5-052b985037b5'
WHERE concept_id = 'unified:9605db13-9baa-5ca2-99e0-1e715fca1c18'
  AND parent_concept_id = 'unified:20d36e61-f10b-52b2-a8d2-bd8356111b24';

DELETE FROM concept
WHERE concept_id = 'unified:20d36e61-f10b-52b2-a8d2-bd8356111b24';

-- Reconcile equivalent criticism and general-literature concepts across DDC,
-- BISAC, and LCC while retaining their detailed children.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:96c1b9c6-90b2-5dbd-ac52-84626372c0cb', 'ddc', '809', 0),
    ('unified:bbcf0a3c-e484-5778-ab84-70596d1c59c9', 'ddc', '800', 0);

DELETE FROM concept
WHERE concept_id IN (
    'unified:75886dcb-58ae-53d8-ba2f-def013c30d71',
    'unified:2f571886-e633-5fba-a687-1f3e25bdfe57'
);

UPDATE concept
SET preferred_label = 'General Literature'
WHERE concept_id = 'unified:bbcf0a3c-e484-5778-ab84-70596d1c59c9';

UPDATE concept
SET preferred_label = 'Literary Theory'
WHERE concept_id = 'unified:3a7aa472-aa2e-5910-9fd8-9a3ef391ad07';

UPDATE concept
SET preferred_label = 'Rhetoric & Literary Collections'
WHERE concept_id = 'unified:c833339a-e53e-557f-aa58-19b907cd90e2';

-- Short reference labels rely on the parent path for context.
UPDATE concept SET preferred_label = 'Dictionaries'
WHERE concept_id = 'unified:05339775-0c69-5385-a065-c5649e12c5d1';

UPDATE concept SET preferred_label = 'Study & Research'
WHERE concept_id = 'unified:35b90b6c-ff04-58df-910e-120635cdb872';

UPDATE concept SET preferred_label = 'Organizations'
WHERE concept_id = 'unified:0e07535f-7283-5aaa-a9d4-099f629e1602';

UPDATE concept SET preferred_label = 'Periodicals'
WHERE concept_id = 'unified:3bb8d821-bcee-52f6-9493-1453fd8ca473';

-- Distinguish BISAC humorous fiction from the separate Humor & Satire branch.
UPDATE concept
SET preferred_label = 'Humorous Fiction'
WHERE concept_id = 'unified:b5fb353b-278e-5383-ae43-c886a52c6b36';

COMMIT;
