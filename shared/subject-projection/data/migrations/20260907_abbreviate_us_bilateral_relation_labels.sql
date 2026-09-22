BEGIN;

UPDATE concept
SET preferred_label='U.S.' || substr(preferred_label,length('United States')+1)
WHERE preferred_label LIKE 'United States–%';

COMMIT;
