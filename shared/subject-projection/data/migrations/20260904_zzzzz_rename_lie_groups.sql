-- Use the recognizable modern name for continuous transformation groups.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='Lie Groups & Transformations'
WHERE concept_id='bokheim:lcc-detail:de458a5c-c032-50cc-9c4d-82cf151967ae';

COMMIT;
