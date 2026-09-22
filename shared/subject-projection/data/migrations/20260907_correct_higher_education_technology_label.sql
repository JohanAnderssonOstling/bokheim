BEGIN;
UPDATE concept SET preferred_label='Higher Education — General Works'
WHERE concept_id=9794 AND preferred_label='Higher Education Technology';
COMMIT;
