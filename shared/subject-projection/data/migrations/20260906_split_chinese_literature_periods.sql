-- Eight direct Chinese Literature children: five author periods and three topical divisions.
-- Retain codes between precise period ranges on the parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:chinese-literature:early','Earlier Literature, through Ming Dynasty');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:early','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,0);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:early','lcc','PL2661..PL2698@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:qing','Qing Dynasty, 1644–1912');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:qing','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,1);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:qing','lcc','PL2699..PL2735@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:1912-1949','1912–1949');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:1912-1949','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,2);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:1912-1949','lcc','PL2735.5..PL2832@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:1949-2000','1949–2000');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:1949-2000','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,3);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:1949-2000','lcc','PL2832.5..PL2929.5@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:2001','2001–');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:2001','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,4);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:2001','lcc','PL2930..PL2979@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:history-criticism','History & Criticism');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:history-criticism','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,5);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:history-criticism','lcc','PL2250..PL2443@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:collections-translations','Collections & Translations');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:collections-translations','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,6);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:collections-translations','lcc','PL2450..PL2659@5',0);
INSERT INTO concept VALUES('bokheim:chinese-literature:regional','Regional Literature');
INSERT INTO concept_parent VALUES('bokheim:chinese-literature:regional','unified:fa2d5cb0-59a1-5836-a632-60ecbac51d80',0,7);
INSERT INTO source_selector VALUES('bokheim:chinese-literature:regional','lcc','PL3030..PL3032.2@5',0);
COMMIT;
