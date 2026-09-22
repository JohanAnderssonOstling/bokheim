-- Remove obsolete Soviet geographic aggregate leaves after countries were
-- split between Eastern Europe and Asia. Unrelated topical subjects remain.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent WHERE concept_id IN (
 'bokheim:lcc-detail:7e4f0ae6-5797-5e85-bcba-dbb458d932b9',
 'bokheim:lcc-detail:15e1a488-e274-5712-b080-59adde8eb4a4'
);
DELETE FROM concept WHERE concept_id IN (
 'bokheim:lcc-detail:7e4f0ae6-5797-5e85-bcba-dbb458d932b9',
 'bokheim:lcc-detail:15e1a488-e274-5712-b080-59adde8eb4a4'
);

UPDATE concept SET preferred_label='Georgia'
WHERE concept_id='bokheim:philosophy-lcc-full:4664b2e6-47fa-585d-a3b8-a9dfd5668e9b';
UPDATE concept SET preferred_label='Armenia'
WHERE concept_id='bokheim:philosophy-lcc-full:1b983129-7334-5f37-b7d4-0fec8cbde411';

COMMIT;
