PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 54. Jewish history and biography.
INSERT INTO concept VALUES ('bokheim:judaism-history:general','History of Judaism—General'),('bokheim:judaism-history:regional','History of Judaism by Country or Region'),('bokheim:judaism-history:biography','Jewish Religious Biography');
INSERT INTO concept_parent VALUES ('bokheim:judaism-history:general','bokheim:subject:judaism:history-community',0,100),('bokheim:judaism-history:regional','bokheim:subject:judaism:history-community',0,200),('bokheim:judaism-history:biography','bokheim:subject:judaism:history-community',0,300);
DELETE FROM source_selector WHERE concept_id='bokheim:subject:judaism:history-community';
INSERT INTO source_selector VALUES ('bokheim:judaism-history:general','bisac','REL040030',0),('bokheim:judaism-history:general','lcc','BM150..BM200.9@3',0),('bokheim:judaism-history:regional','lcc','BM201..BM449@3',0),('bokheim:judaism-history:biography','lcc','BM750..BM755@3',0);

COMMIT;
