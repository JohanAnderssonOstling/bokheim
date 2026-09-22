-- Approved eight journalism-topic extractions, 2026-09-07.
-- Keep Practical Journalism as residual coverage.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES(52365,'Reporting & Correspondence');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52365,12544,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52365,'PN4781');
INSERT INTO concept(concept_id,preferred_label) VALUES(52366,'Broadcast Journalism');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52366,12544,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52366,'PN4784.B75');
INSERT INTO concept(concept_id,preferred_label) VALUES(52367,'Television Journalism');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52367,52366,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52367,'PN4784.T4');
INSERT INTO concept(concept_id,preferred_label) VALUES(52368,'Online Journalism');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52368,12544,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52368,'PN4784.O62');
INSERT INTO concept(concept_id,preferred_label) VALUES(52369,'Newspaper Style');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52369,12544,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52369,'PN4783');
INSERT INTO concept(concept_id,preferred_label) VALUES(52370,'Journalistic Editing');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52370,12544,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52370,'PN4778');
INSERT INTO concept(concept_id,preferred_label) VALUES(52371,'War Reporting');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52371,52365,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52371,'PN4784.W37');
INSERT INTO concept(concept_id,preferred_label) VALUES(52372,'Sports Journalism');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52372,52365,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52372,'PN4784.S6');
COMMIT;
