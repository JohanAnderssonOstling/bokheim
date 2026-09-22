-- Shorten spelled-out century names in the curated History taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='17th Century, 1601–1715'
 WHERE concept_id=3668 AND preferred_label='Seventeenth Century, 1601–1715';
INSERT INTO source_label(concept_id,label,ordinal)
 SELECT 3668,'Seventeenth Century, 1601–1715',0
 WHERE changes() > 0;

UPDATE concept SET preferred_label='18th Century, 1715–1789'
 WHERE concept_id=3669 AND preferred_label='Eighteenth Century, 1715–1789';
INSERT INTO source_label(concept_id,label,ordinal)
 SELECT 3669,'Eighteenth Century, 1715–1789',0
 WHERE changes() > 0;

UPDATE concept SET preferred_label='20th Century'
 WHERE concept_id=11557 AND preferred_label='Twentieth Century';
INSERT INTO source_label(concept_id,label,ordinal)
 SELECT 11557,'Twentieth Century',0
 WHERE changes() > 0;

UPDATE concept SET preferred_label='19th Century'
 WHERE concept_id=11558 AND preferred_label='Nineteenth Century';
INSERT INTO source_label(concept_id,label,ordinal)
 SELECT 11558,'Nineteenth Century',0
 WHERE changes() > 0;

COMMIT;
