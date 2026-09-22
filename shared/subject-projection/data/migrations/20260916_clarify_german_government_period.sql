-- LC JN3357-3444 includes the North German Confederation as well as the Empire.
-- Label-only follow-up to the Politics & Government review; no code changes.
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label='Confederation & Empire, 1867–1918'
WHERE concept_id=52866 AND preferred_label='German Empire, 1867–1918';
COMMIT;
