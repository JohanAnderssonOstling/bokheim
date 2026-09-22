-- Normalize approximate period labels while preserving all source/code ranges.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='1500–1700' WHERE concept_id=12703;
UPDATE concept SET preferred_label='1700–1860' WHERE concept_id=4245;
UPDATE concept SET preferred_label='1700–1868' WHERE concept_id=12714;
UPDATE concept SET preferred_label='1860–1960' WHERE concept_id=12704;
UPDATE concept SET preferred_label='19th C., 1770–1890' WHERE concept_id=3915;
UPDATE concept SET preferred_label='1050–1450' WHERE concept_id=12702;
UPDATE concept SET preferred_label='To 1810' WHERE concept_id IN (11950,11955,11964,11977);

COMMIT;
