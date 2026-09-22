-- Use the period name rather than the source date notation.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='17th & 18th Century'
WHERE concept_id=3912;

COMMIT;
