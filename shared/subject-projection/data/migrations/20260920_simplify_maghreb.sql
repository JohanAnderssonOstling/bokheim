BEGIN;
UPDATE concept SET preferred_label = 'Maghreb' WHERE concept_id = 3349;
DELETE FROM source_label WHERE concept_id = 3349;
COMMIT;
