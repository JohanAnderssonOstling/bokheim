-- Shorten the Literature branch label while preserving its children and
-- selector coverage. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Genres'
WHERE concept_id = 'bokheim:subject:literature:genres';

COMMIT;
