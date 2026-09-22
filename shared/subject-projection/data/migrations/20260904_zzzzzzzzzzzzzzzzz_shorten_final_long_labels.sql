-- Shorten the final two long labels using their complete LCC source captions.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='Eastern Church Canon Law: Other Subjects'
WHERE concept_id='unified:58a9fe8b-c537-567b-9437-aad4db49e6bf';

UPDATE concept
SET preferred_label='Destruction or Attempted Destruction of a Body'
WHERE concept_id='bokheim:lcc-detail:bcef802a-e1c6-53f4-be47-49c7ad564001';

COMMIT;
